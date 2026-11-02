//! Raw Parquet file layout: `raw/dt=YYYY-MM-DD/HH[_n].parquet`.

use std::path::{Path, PathBuf};
use time::{Date, Month, OffsetDateTime};

use super::schema::FLOW_COLUMNS;
use crate::StoreResult;

pub const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;

/// One raw partition file with the start of the hour it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFile {
    pub path: PathBuf,
    pub hour_ms: i64,
    pub bytes: u64,
}

pub fn hour_start(ts_ms: i64) -> i64 {
    ts_ms - ts_ms.rem_euclid(HOUR_MS)
}

/// `(dt=YYYY-MM-DD dir name, HH)` for an hour start.
pub fn partition(hour_ms: i64) -> (String, String) {
    let dt = OffsetDateTime::from_unix_timestamp(hour_ms.div_euclid(1000))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let d = dt.date();
    (
        format!(
            "dt={:04}-{:02}-{:02}",
            d.year(),
            u8::from(d.month()),
            d.day()
        ),
        format!("{:02}", dt.hour()),
    )
}

/// First unused path for an hour (`HH.parquet`, then `HH_1.parquet`, …).
pub fn next_path(raw_dir: &Path, hour_ms: i64) -> PathBuf {
    let (dt, hh) = partition(hour_ms);
    let dir = raw_dir.join(dt);
    let base = dir.join(format!("{hh}.parquet"));
    if !base.exists() {
        return base;
    }
    (1..)
        .map(|n| dir.join(format!("{hh}_{n}.parquet")))
        .find(|p| !p.exists())
        .unwrap_or(base)
}

fn parse_day(name: &str) -> Option<i64> {
    let s = name.strip_prefix("dt=")?;
    let mut it = s.split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    let d: u8 = it.next()?.parse().ok()?;
    let date = Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()?;
    Some(date.midnight().assume_utc().unix_timestamp() * 1000)
}

fn parse_hour(name: &str) -> Option<i64> {
    let stem = name.strip_suffix(".parquet")?;
    let hh: i64 = stem.split('_').next()?.parse().ok()?;
    (0..24).contains(&hh).then_some(hh * HOUR_MS)
}

/// Raw files whose hour overlaps `[from_ms, to_ms)`, oldest first.
pub fn raw_files(raw_dir: &Path, from_ms: i64, to_ms: i64) -> StoreResult<Vec<RawFile>> {
    let mut out = Vec::new();
    if !raw_dir.exists() {
        return Ok(out);
    }
    for day in std::fs::read_dir(raw_dir)? {
        let day = day?;
        let Some(day_ms) = day.file_name().to_str().and_then(parse_day) else {
            continue;
        };
        if day_ms >= to_ms || day_ms + DAY_MS <= from_ms {
            continue;
        }
        for f in std::fs::read_dir(day.path())? {
            let f = f?;
            let Some(h) = f.file_name().to_str().and_then(parse_hour) else {
                continue;
            };
            let hour_ms = day_ms + h;
            if hour_ms < to_ms && hour_ms + HOUR_MS > from_ms {
                out.push(RawFile {
                    path: f.path(),
                    hour_ms,
                    bytes: f.metadata()?.len(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.hour_ms.cmp(&b.hour_ms).then(a.path.cmp(&b.path)));
    Ok(out)
}

fn quote(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

/// Parquet rows under the current flow schema. Files written before a column
/// existed (e.g. `remote_host_source`, feature 002) read it as NULL: the files
/// are unioned BY NAME with an empty, fully typed row set. `options` is
/// appended to the `read_parquet` arguments (e.g. `, file_row_number = true`).
pub fn parquet_rows(paths: &[&Path], options: &str) -> String {
    let list: Vec<String> = paths.iter().map(|p| quote(p)).collect();
    format!(
        "(SELECT * FROM (SELECT {FLOW_COLUMNS} FROM flows_hot LIMIT 0) UNION ALL BY NAME \
         SELECT * FROM read_parquet([{}], union_by_name = true{options}))",
        list.join(", ")
    )
}

/// SQL for the union of the hot table and the given Parquet files.
pub fn union_sql(files: &[RawFile]) -> String {
    let hot = format!("SELECT {FLOW_COLUMNS} FROM flows_hot");
    if files.is_empty() {
        return hot;
    }
    let paths: Vec<&Path> = files.iter().map(|f| f.path.as_path()).collect();
    format!(
        "{hot} UNION ALL SELECT {FLOW_COLUMNS} FROM {}",
        parquet_rows(&paths, "")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partitions_and_parsing() {
        let h = 1_767_225_600_000 + 5 * HOUR_MS;
        assert_eq!(partition(h), ("dt=2026-01-01".into(), "05".into()));
        assert_eq!(parse_day("dt=2026-01-01"), Some(1_767_225_600_000));
        assert_eq!(parse_hour("05_2.parquet"), Some(5 * HOUR_MS));
        assert_eq!(parse_hour("25.parquet"), None);
        assert_eq!(hour_start(h + 1234), h);
        assert!(union_sql(&[]).ends_with("flows_hot"));
    }
}
