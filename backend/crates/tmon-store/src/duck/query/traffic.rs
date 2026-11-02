//! Traffic time series and breakdowns (FR-014).

use duckdb::{Connection, params_from_iter};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use tmon_core::rules::Confidence;

use super::labels::{direction_label, proto_name};
use super::router::{Dim, Filter, Source, Windows, route};
use super::sql::Where;
use crate::StoreResult;

/// Target number of points when `step` is not given.
pub const TARGET_POINTS: i64 = 200;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Series {
    pub key: String,
    pub label: String,
    pub points: Vec<(i64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimeSeries {
    /// Bucket width in seconds.
    pub step: i64,
    pub series: Vec<Series>,
    #[serde(skip)]
    pub source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BreakdownRow {
    pub key: String,
    pub label: String,
    pub bytes: u64,
    pub packets: u64,
    pub flows: u64,
    pub share: f64,
    /// `by=rule` only: how sure the categorization is (`detected`/`inferred`/`guessed`/`none`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// `by=app` only: share of the row's flows classified confidently.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confident_share: Option<f64>,
}

/// Key of [`Dim::Rule`] rows for flows no rule matched.
pub const NO_RULE_KEY: &str = "0";

/// SQL expression for a dimension key (as text) on a source.
pub fn key_expr(dim: Dim, raw: bool) -> &'static str {
    match (dim, raw) {
        (Dim::Category, _) => "CAST(category_id AS VARCHAR)",
        (Dim::Direction, _) => "direction",
        (Dim::Proto, _) => "CAST(proto AS VARCHAR)",
        (Dim::Country, true) => "coalesce(country, '')",
        (Dim::Country, false) => "country",
        (Dim::Asn, true) => "CAST(coalesce(asn, 0) AS VARCHAR)",
        (Dim::Asn, false) => "CAST(asn AS VARCHAR)",
        (Dim::Exporter, _) => "CAST(exporter_id AS VARCHAR)",
        (Dim::Host, _) => "local_ip",
        (Dim::Port, _) => "CAST(remote_port AS VARCHAR)",
        (Dim::RemoteIp, _) => "remote_ip",
        (Dim::Rule, true) => "CAST(coalesce(rule_id, 0) AS VARCHAR)",
        (Dim::Rule, false) => "CAST(rule_id AS VARCHAR)",
        (Dim::App, _) => "coalesce(app, '')",
    }
}

/// Resolve display labels for keys of a dimension.
pub fn label_map(conn: &Connection, dim: Dim) -> StoreResult<BTreeMap<String, String>> {
    let sql = match dim {
        Dim::Category => "SELECT CAST(id AS VARCHAR), name FROM category",
        Dim::Asn => "SELECT CAST(asn AS VARCHAR), name FROM asn_name",
        Dim::Exporter => "SELECT CAST(id AS VARCHAR), coalesce(name, address) FROM exporter",
        Dim::Host => "SELECT ip, coalesce(friendly_name, dhcp_hostname, ip) FROM local_host",
        Dim::Rule => "SELECT CAST(id AS VARCHAR), name FROM rule",
        _ => return Ok(BTreeMap::new()),
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn label(dim: Dim, key: &str, map: &BTreeMap<String, String>) -> String {
    if let Some(l) = map.get(key) {
        return l.clone();
    }
    match dim {
        Dim::Proto => key
            .parse()
            .map(proto_name)
            .unwrap_or_else(|_| key.to_string()),
        Dim::Direction => direction_label(key),
        Dim::Country if key.is_empty() => "Unknown".into(),
        Dim::Asn if key == "0" => "Unknown".into(),
        Dim::Asn => format!("AS{key}"),
        Dim::Rule if key == NO_RULE_KEY => "Unmatched".into(),
        Dim::Rule => format!("Deleted rule #{key}"),
        Dim::App if key.is_empty() => "Unclassified".into(),
        _ => key.to_string(),
    }
}

/// Bucket width: requested step rounded up to the source grain, or ~200 points.
pub fn pick_step(f: &Filter, src: &Source, requested_ms: Option<i64>) -> i64 {
    let grain = src.grain_ms().max(1);
    let want = requested_ms.unwrap_or(((f.to_ms - f.from_ms) / TARGET_POINTS).max(1));
    ((want + grain - 1) / grain).max(1) * grain
}

pub fn timeseries(
    conn: &Connection,
    raw_dir: &Path,
    f: &Filter,
    group: Dim,
    step_ms: Option<i64>,
    w: Windows,
    now: i64,
) -> StoreResult<TimeSeries> {
    let src = route(f, &[group], w, now)?;
    let step = pick_step(f, &src, step_ms);
    let wh = Where::from_filter(f, &src);
    let t = src.time_column();
    let sql = format!(
        "SELECT {t} - ({t} % {step}) AS b, {k} AS k, sum(bytes)::UBIGINT FROM {from}{w} GROUP BY ALL ORDER BY b",
        k = key_expr(group, src.is_raw()),
        from = src.from_sql(raw_dir, f)?,
        w = wh.sql()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(wh.params()), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u64>(2)?,
        ))
    })?;
    let mut by_key: BTreeMap<String, BTreeMap<i64, u64>> = BTreeMap::new();
    for row in rows {
        let (b, k, bytes) = row?;
        *by_key.entry(k).or_default().entry(b).or_default() += bytes;
    }
    let labels = label_map(conn, group)?;
    let first = f.from_ms - f.from_ms.rem_euclid(step);
    let series = by_key
        .into_iter()
        .map(|(key, pts)| Series {
            label: label(group, &key, &labels),
            points: (0..)
                .map(|i| first + i * step)
                .take_while(|b| *b < f.to_ms)
                .map(|b| (b, pts.get(&b).copied().unwrap_or(0)))
                .collect(),
            key,
        })
        .collect();
    Ok(TimeSeries {
        step: step / 1000,
        series,
        source: src.label(),
    })
}

pub fn breakdown(
    conn: &Connection,
    raw_dir: &Path,
    f: &Filter,
    by: Dim,
    w: Windows,
    now: i64,
) -> StoreResult<(Vec<BreakdownRow>, &'static str)> {
    let src = route(f, &[by], w, now)?;
    let wh = Where::from_filter(f, &src);
    let flows = if src.is_raw() {
        "count(*)"
    } else {
        "sum(flows)"
    };
    // Dim::App routes to raw only, where `app_confident` exists.
    let confident = if by == Dim::App {
        "avg(CASE WHEN app_confident THEN 1.0 ELSE 0.0 END)"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT {k} AS k, sum(bytes)::UBIGINT AS b, sum(packets)::UBIGINT, {flows}::UBIGINT, {confident}::DOUBLE FROM {from}{w} GROUP BY ALL ORDER BY b DESC",
        k = key_expr(by, src.is_raw()),
        from = src.from_sql(raw_dir, f)?,
        w = wh.sql()
    );
    let rows: Vec<(String, u64, u64, u64, Option<f64>)> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(wh.params()), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<_, _>>()?;
    let total: u64 = rows.iter().map(|r| r.1).sum();
    let labels = label_map(conn, by)?;
    let confidence = if by == Dim::Rule {
        rule_confidence(conn)?
    } else {
        BTreeMap::new()
    };
    let out = rows
        .into_iter()
        .map(
            |(key, bytes, packets, flows, confident_share)| BreakdownRow {
                label: label(by, &key, &labels),
                share: if total == 0 {
                    0.0
                } else {
                    bytes as f64 / total as f64
                },
                confidence: (by == Dim::Rule).then(|| {
                    confidence
                        .get(&key)
                        .copied()
                        .unwrap_or(Confidence::None)
                        .as_str()
                        .to_string()
                }),
                key,
                bytes,
                packets,
                flows,
                confident_share,
            },
        )
        .collect();
    Ok((out, src.label()))
}

/// Confidence per rule id (as breakdown keys), from each rule's match definition.
fn rule_confidence(conn: &Connection) -> StoreResult<BTreeMap<String, Confidence>> {
    Ok(crate::duck::repo::rules::list(conn)?
        .into_iter()
        .map(|r| (r.id.to_string(), r.match_.confidence()))
        .collect())
}
