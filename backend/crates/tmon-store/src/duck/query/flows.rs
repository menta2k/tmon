//! Raw flow search and CSV export source (FR-018).

use duckdb::{Connection, params_from_iter};
use serde::Serialize;
use std::path::Path;

pub use super::flow_sort::{PageReq, Sort, SortKey};
use super::router::{Filter, Source, Windows};
use super::sql::Where;
use crate::duck::rows::record;
use crate::duck::schema::FLOW_COLUMNS;
use crate::{StoreError, StoreResult};

pub const MAX_PAGE: u64 = 500;
pub const EXPORT_CAP: usize = 100_000;

/// API view of a stored flow (`ts`/`ts_start` in epoch ms).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlowRow {
    pub ts: i64,
    pub ts_start: i64,
    pub exporter_id: u16,
    pub in_if: u32,
    pub out_if: u32,
    pub ip_ver: u8,
    pub local_ip: String,
    pub remote_ip: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub proto: u8,
    pub tcp_flags: u8,
    pub bytes: u64,
    pub packets: u64,
    pub sampled: bool,
    pub direction: &'static str,
    pub nat_ip: Option<String>,
    pub nat_port: Option<u16>,
    pub category_id: u16,
    pub rule_id: Option<u32>,
    pub country: Option<String>,
    pub asn: Option<u32>,
    pub as_name: Option<String>,
    pub remote_host: Option<String>,
    /// `tls|quic|http|dns|router`, null when unknown (feature 002).
    pub remote_host_source: Option<&'static str>,
    /// Classifier application (feature 003), null when unclassified.
    pub app: Option<String>,
    /// Whether the classifier was sure; null when unclassified.
    pub app_confident: Option<bool>,
}

pub const CSV_HEADER: [&str; 27] = [
    "ts",
    "ts_start",
    "exporter_id",
    "in_if",
    "out_if",
    "ip_ver",
    "local_ip",
    "remote_ip",
    "local_port",
    "remote_port",
    "proto",
    "tcp_flags",
    "bytes",
    "packets",
    "sampled",
    "direction",
    "nat_ip",
    "nat_port",
    "category_id",
    "rule_id",
    "country",
    "asn",
    "as_name",
    "remote_host",
    "remote_host_source",
    "app",
    "app_confident",
];

impl FlowRow {
    fn from_record(r: tmon_core::FlowRecord) -> Self {
        let s = |v: Option<std::sync::Arc<str>>| v.map(|x| x.to_string());
        Self {
            ts: r.ts_ms,
            ts_start: r.ts_start_ms,
            exporter_id: r.exporter_id,
            in_if: r.in_if,
            out_if: r.out_if,
            ip_ver: r.ip_ver,
            local_ip: r.local_ip.to_string(),
            remote_ip: r.remote_ip.to_string(),
            local_port: r.local_port,
            remote_port: r.remote_port,
            proto: r.proto,
            tcp_flags: r.tcp_flags,
            bytes: r.bytes,
            packets: r.packets,
            sampled: r.sampled,
            direction: r.direction.as_str(),
            nat_ip: r.nat_ip.map(|i| i.to_string()),
            nat_port: r.nat_port,
            category_id: r.category_id,
            rule_id: r.rule_id,
            country: s(r.country),
            asn: r.asn,
            as_name: s(r.as_name),
            remote_host: s(r.remote_host),
            remote_host_source: r.remote_host_source.map(|s| s.as_str()),
            app_confident: r.app.is_some().then_some(r.app_confident),
            app: s(r.app),
        }
    }

    /// CSV fields in `CSV_HEADER` order.
    pub fn csv_fields(&self) -> Vec<String> {
        let o = |v: Option<String>| v.map(|t| neutralize(&t)).unwrap_or_default();
        let n = |v: Option<u32>| v.map(|x| x.to_string()).unwrap_or_default();
        vec![
            self.ts.to_string(),
            self.ts_start.to_string(),
            self.exporter_id.to_string(),
            self.in_if.to_string(),
            self.out_if.to_string(),
            self.ip_ver.to_string(),
            self.local_ip.clone(),
            self.remote_ip.clone(),
            self.local_port.to_string(),
            self.remote_port.to_string(),
            self.proto.to_string(),
            self.tcp_flags.to_string(),
            self.bytes.to_string(),
            self.packets.to_string(),
            self.sampled.to_string(),
            self.direction.to_string(),
            o(self.nat_ip.clone()),
            self.nat_port.map(|p| p.to_string()).unwrap_or_default(),
            self.category_id.to_string(),
            n(self.rule_id),
            o(self.country.clone()),
            n(self.asn),
            o(self.as_name.clone()),
            o(self.remote_host.clone()),
            self.remote_host_source.unwrap_or_default().to_string(),
            o(self.app.clone()),
            self.app_confident
                .map(|c| c.to_string())
                .unwrap_or_default(),
        ]
    }
}

/// Prefix text that a spreadsheet would treat as a formula (CSV injection).
/// Applied to free-text columns (names, hostnames) — never to numbers or IPs.
pub fn neutralize(v: &str) -> String {
    match v.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{v}"),
        _ => v.to_string(),
    }
}

/// Raw data only exists within raw retention.
fn check_range(f: &Filter, w: Windows, now_ms: i64) -> StoreResult<()> {
    if f.from_ms < now_ms - w.raw_ms {
        return Err(StoreError::UnsupportedFilterRange {
            supported_dims: Vec::new(),
        });
    }
    Ok(())
}

fn base(raw_dir: &Path, f: &Filter) -> StoreResult<(String, Where)> {
    let src = Source::Raw;
    Ok((src.from_sql(raw_dir, f)?, Where::from_filter(f, &src)))
}

/// One page in `req.sort` order, plus the total match count.
pub fn search(
    conn: &Connection,
    raw_dir: &Path,
    f: &Filter,
    req: PageReq,
    w: Windows,
    now_ms: i64,
) -> StoreResult<(Vec<FlowRow>, u64)> {
    check_range(f, w, now_ms)?;
    let limit = req.limit.clamp(1, MAX_PAGE);
    let (from, wh) = base(raw_dir, f)?;
    let total: u64 = conn.query_row(
        &format!("SELECT count(*)::UBIGINT FROM {from}{}", wh.sql()),
        params_from_iter(wh.params()),
        |r| r.get(0),
    )?;
    let offset = req.page.saturating_sub(1).saturating_mul(limit);
    let sql = format!(
        "SELECT {FLOW_COLUMNS} FROM {from}{} ORDER BY {} LIMIT {limit} OFFSET {offset}",
        wh.sql(),
        req.sort.order_by()
    );
    let rows = conn
        .prepare(&sql)?
        .query_map(params_from_iter(wh.params()), |r| {
            record(r, 0).map(FlowRow::from_record)
        })?
        .collect::<Result<_, _>>()?;
    Ok((rows, total))
}

/// Visit up to `cap` matching rows in `sort` order; returns `(written, truncated)`.
pub fn export(
    conn: &Connection,
    raw_dir: &Path,
    f: &Filter,
    (cap, sort): (usize, Sort),
    w: Windows,
    now_ms: i64,
    mut visit: impl FnMut(&FlowRow) -> StoreResult<()>,
) -> StoreResult<(usize, bool)> {
    check_range(f, w, now_ms)?;
    let (from, wh) = base(raw_dir, f)?;
    let sql = format!(
        "SELECT {FLOW_COLUMNS} FROM {from}{} ORDER BY {} LIMIT {}",
        wh.sql(),
        sort.order_by(),
        cap + 1
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(wh.params()))?;
    let mut n = 0;
    while let Some(r) = rows.next()? {
        if n == cap {
            return Ok((n, true));
        }
        visit(&FlowRow::from_record(record(r, 0)?))?;
        n += 1;
    }
    Ok((n, false))
}

#[cfg(test)]
mod tests {
    use super::neutralize;

    #[test]
    fn neutralizes_formula_prefixes() {
        assert_eq!(neutralize("=HYPERLINK(1)"), "'=HYPERLINK(1)");
        assert_eq!(neutralize("-x.example"), "'-x.example");
        assert_eq!(neutralize("GOOGLE"), "GOOGLE");
        assert_eq!(neutralize(""), "");
    }
}
