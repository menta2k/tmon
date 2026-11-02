//! Re-apply categorization to stored history (FR-023).

use duckdb::{Connection, params};
use std::path::Path;
use tmon_core::Categorizer;

use super::DuckStore;
use super::files::{HOUR_MS, RawFile, hour_start, parquet_rows, raw_files};
use super::ingest::rollup_sql;
use super::rows::record;
use super::schema::{FLOW_COLUMNS, GRAINS, ROLLUPS};
use crate::StoreResult;

pub(crate) const MAP_DDL: &str = "CREATE OR REPLACE TEMP TABLE recat_map (rn BIGINT PRIMARY KEY, category_id USMALLINT NOT NULL, rule_id UINTEGER)";

fn quote(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

/// Stream rows of `select` (first column = row key, then FLOW_COLUMNS) into `recat_map`.
pub(crate) fn build_map(
    conn: &Connection,
    select: &str,
    cat: &dyn Categorizer,
) -> StoreResult<usize> {
    conn.execute_batch(MAP_DDL)?;
    let mut stmt = conn.prepare(select)?;
    let mut rows = stmt.query([])?;
    let mut app = conn.appender("recat_map")?;
    let mut changed = 0;
    while let Some(r) = rows.next()? {
        let key: i64 = r.get(0)?;
        let rec = record(r, 1)?;
        let (c, rule) = cat.categorize(&rec);
        if (c, rule) != (rec.category_id, rec.rule_id) {
            changed += 1;
        }
        app.append_row(params![key, c, rule])?;
    }
    app.flush()?;
    Ok(changed)
}

/// Rewrite one Parquet file with new categories (temp file + atomic rename).
fn rewrite_file(conn: &Connection, f: &RawFile, cat: &dyn Categorizer) -> StoreResult<usize> {
    let src = parquet_rows(&[f.path.as_path()], ", file_row_number = true");
    let changed = build_map(
        conn,
        &format!("SELECT file_row_number, {FLOW_COLUMNS} FROM {src}"),
        cat,
    )?;
    if changed == 0 {
        return Ok(0);
    }
    let tmp = f.path.with_extension("parquet.tmp");
    let copy = format!(
        "COPY (SELECT {cols} FROM (SELECT p.* REPLACE (m.category_id AS category_id, m.rule_id AS rule_id) \
         FROM {src} p JOIN recat_map m ON m.rn = p.file_row_number ORDER BY p.file_row_number)) \
         TO {tmp} (FORMAT parquet, COMPRESSION zstd)",
        cols = FLOW_COLUMNS,
        tmp = quote(&tmp)
    );
    if let Err(e) = conn.execute_batch(&copy) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    std::fs::rename(&tmp, &f.path)?;
    Ok(changed)
}

fn rewrite_hot(conn: &Connection, hour: i64, cat: &dyn Categorizer) -> StoreResult<usize> {
    let sel = format!(
        "SELECT rowid, {FLOW_COLUMNS} FROM flows_hot WHERE ts >= {hour} AND ts < {}",
        hour + HOUR_MS
    );
    let changed = build_map(conn, &sel, cat)?;
    if changed > 0 {
        conn.execute_batch(
            "UPDATE flows_hot SET category_id = m.category_id, rule_id = m.rule_id FROM recat_map m WHERE flows_hot.rowid = m.rn",
        )?;
    }
    Ok(changed)
}

/// Delete and rebuild the 1m/1h category-dependent rollups of one hour.
fn rebuild_rollups(conn: &Connection, hour: i64, files: &[RawFile]) -> StoreResult<()> {
    let end = hour + HOUR_MS;
    let mut src = format!("SELECT {FLOW_COLUMNS} FROM flows_hot WHERE ts >= {hour} AND ts < {end}");
    for f in files {
        src.push_str(&format!(
            " UNION ALL SELECT {FLOW_COLUMNS} FROM {} WHERE ts >= {hour} AND ts < {end}",
            parquet_rows(&[f.path.as_path()], "")
        ));
    }
    let tx = conn.unchecked_transaction()?;
    for (family, _, _) in ROLLUPS {
        for (grain, _) in GRAINS {
            tx.execute(
                &format!("DELETE FROM {family}_{grain} WHERE bucket >= ? AND bucket < ?"),
                params![hour, end],
            )?;
        }
    }
    tx.execute_batch(&rollup_sql(&format!("({src}) AS recat_src")))?;
    tx.commit()?;
    Ok(())
}

/// Recategorize `[from_ms, to_ms)` hour by hour; `progress(0..=1)` after each hour.
pub fn recategorize(
    store: &DuckStore,
    from_ms: i64,
    to_ms: i64,
    cat: &dyn Categorizer,
    progress: &dyn Fn(f64),
) -> StoreResult<usize> {
    let first = hour_start(from_ms);
    let hours: Vec<i64> = (0..)
        .map(|i| first + i * HOUR_MS)
        .take_while(|h| *h < to_ms)
        .collect();
    let total = hours.len().max(1) as f64;
    let mut changed = 0;
    progress(0.0);
    for (i, hour) in hours.iter().enumerate() {
        changed += store.with_writer(|c| {
            let files = raw_files(store.raw_dir(), *hour, hour + HOUR_MS)?;
            let mut n = rewrite_hot(c, *hour, cat)?;
            for f in &files {
                n += rewrite_file(c, f, cat)?;
            }
            if n > 0 {
                rebuild_rollups(c, *hour, &files)?;
            }
            c.execute_batch("DROP TABLE IF EXISTS recat_map")?;
            Ok(n)
        })?;
        progress((i + 1) as f64 / total);
    }
    Ok(changed)
}
