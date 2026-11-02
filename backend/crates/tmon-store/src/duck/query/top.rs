//! Top-N queries (FR-015): hosts, remotes, ASNs, countries, ports.

use duckdb::{Connection, params_from_iter};
use serde::Serialize;
use std::path::Path;

use super::router::{Dim, Filter, Source, Windows, route};
use super::sql::Where;
use super::traffic::{label, label_map};
use crate::{StoreError, StoreResult};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopHost {
    pub ip: String,
    pub name: Option<String>,
    pub bytes_down: u64,
    pub bytes_up: u64,
    pub flows: u64,
    pub top_category: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopRemote {
    pub ip: String,
    pub hostname: Option<String>,
    /// Where `hostname` came from (latest raw flow or the shared name list);
    /// null when only rollups were read.
    pub hostname_source: Option<&'static str>,
    pub country: Option<String>,
    pub asn: Option<u32>,
    pub as_name: Option<String>,
    pub bytes: u64,
    pub flows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopItem {
    pub key: String,
    pub label: String,
    pub bytes: u64,
    pub flows: u64,
}

pub fn flows_expr(src: &Source) -> &'static str {
    if src.is_raw() {
        "count(*)"
    } else {
        "sum(flows)"
    }
}

/// Owned query context to keep signatures short.
pub struct Ctx<'a> {
    pub conn: &'a Connection,
    pub raw_dir: &'a Path,
    pub windows: Windows,
    pub now_ms: i64,
}

impl Ctx<'_> {
    fn source(&self, f: &Filter, group: &[Dim]) -> StoreResult<(Source, String, Where)> {
        let src = route(f, group, self.windows, self.now_ms)?;
        let from = src.from_sql(self.raw_dir, f)?;
        let w = Where::from_filter(f, &src);
        Ok((src, from, w))
    }
}

pub fn top_hosts(cx: &Ctx, f: &Filter, limit: u64) -> StoreResult<(Vec<TopHost>, &'static str)> {
    let (src, from, w) = cx.source(f, &[Dim::Host, Dim::Direction, Dim::Category])?;
    let sql = format!(
        "WITH t AS (SELECT local_ip AS ip, direction AS d, category_id AS c, sum(bytes) AS b, {fl} AS n FROM {from}{w} GROUP BY ALL), \
         c AS (SELECT ip, arg_max(c, cb) AS top FROM (SELECT ip, c, sum(b) AS cb FROM t GROUP BY ALL) GROUP BY ip) \
         SELECT t.ip, any_value(coalesce(h.friendly_name, h.dhcp_hostname)), \
           coalesce(sum(t.b) FILTER (WHERE t.d = 'down'), 0)::UBIGINT, coalesce(sum(t.b) FILTER (WHERE t.d = 'up'), 0)::UBIGINT, \
           sum(t.n)::UBIGINT, any_value(c.top) \
         FROM t JOIN c USING (ip) LEFT JOIN local_host h ON h.ip = t.ip GROUP BY t.ip ORDER BY sum(t.b) DESC, t.ip LIMIT {limit}",
        fl = flows_expr(&src),
        w = w.sql()
    );
    let rows = cx
        .conn
        .prepare(&sql)?
        .query_map(params_from_iter(w.params()), |r| {
            Ok(TopHost {
                ip: r.get(0)?,
                name: r.get(1)?,
                bytes_down: r.get(2)?,
                bytes_up: r.get(3)?,
                flows: r.get(4)?,
                top_category: r.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok((rows, src.label()))
}

/// Dims answerable from `rollup_remote_1h` beyond raw retention.
const REMOTE_ROLLUP_DIMS: [Dim; 4] = [Dim::Exporter, Dim::Direction, Dim::Host, Dim::RemoteIp];

fn remote_source(cx: &Ctx, f: &Filter) -> StoreResult<(&'static str, String, Where)> {
    if f.from_ms >= cx.now_ms - cx.windows.raw_ms {
        let src = Source::Raw;
        return Ok((
            "raw",
            src.from_sql(cx.raw_dir, f)?,
            Where::from_filter(f, &src),
        ));
    }
    if !f.dims().iter().all(|d| REMOTE_ROLLUP_DIMS.contains(d)) {
        let supported = REMOTE_ROLLUP_DIMS
            .iter()
            .map(|d| d.as_str().to_string())
            .collect();
        return Err(StoreError::UnsupportedFilterRange {
            supported_dims: supported,
        });
    }
    let src = Source::Rollup {
        family: "rollup_remote",
        grain: super::router::Grain::Hour,
    };
    Ok((
        "rollup_1h",
        "rollup_remote_1h".into(),
        Where::from_filter(f, &src),
    ))
}

pub fn top_remotes(
    cx: &Ctx,
    f: &Filter,
    limit: u64,
) -> StoreResult<(Vec<TopRemote>, &'static str)> {
    let (label, from, w) = remote_source(cx, f)?;
    let flows = if label == "raw" {
        "count(*)"
    } else {
        "sum(flows)"
    };
    let source = if label == "raw" {
        "coalesce(arg_max(t.remote_host_source, t.ts) FILTER (WHERE t.remote_host IS NOT NULL), \
           CASE WHEN any_value(t.remote_host) IS NULL THEN \
             CASE any_value(rh.source) WHEN 'capture' THEN 4 WHEN 'dns_cache' THEN 5 END END)"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT t.remote_ip, coalesce(any_value(t.remote_host), any_value(rh.hostname)), any_value(t.country), any_value(t.asn), \
           any_value(t.as_name), sum(t.bytes)::UBIGINT AS b, {flows}::UBIGINT, ({source})::UTINYINT \
         FROM (SELECT * FROM {from}{w}) t LEFT JOIN remote_hostname rh ON rh.ip = t.remote_ip \
         GROUP BY t.remote_ip ORDER BY b DESC, t.remote_ip LIMIT {limit}",
        w = w.sql()
    );
    let rows = cx
        .conn
        .prepare(&sql)?
        .query_map(params_from_iter(w.params()), |r| {
            Ok(TopRemote {
                ip: r.get(0)?,
                hostname: r.get(1)?,
                hostname_source: r
                    .get::<_, Option<u8>>(7)?
                    .and_then(tmon_core::NameSource::from_code)
                    .map(|s| s.as_str()),
                country: r.get::<_, Option<String>>(2)?.filter(|c| !c.is_empty()),
                asn: r.get::<_, Option<u32>>(3)?.filter(|a| *a != 0),
                as_name: r.get(4)?,
                bytes: r.get(5)?,
                flows: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok((rows, label))
}

/// Generic top-N over one dimension (asn, country, port).
pub fn top_by(
    cx: &Ctx,
    f: &Filter,
    dim: Dim,
    limit: u64,
) -> StoreResult<(Vec<TopItem>, &'static str)> {
    let (src, from, w) = cx.source(f, &[dim])?;
    let key = super::traffic::key_expr(dim, src.is_raw());
    let sql = format!(
        "SELECT {key} AS k, sum(bytes)::UBIGINT AS b, {fl}::UBIGINT FROM {from}{w} GROUP BY ALL ORDER BY b DESC, k LIMIT {limit}",
        fl = flows_expr(&src),
        w = w.sql()
    );
    let rows: Vec<(String, u64, u64)> = cx
        .conn
        .prepare(&sql)?
        .query_map(params_from_iter(w.params()), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    let labels = label_map(cx.conn, dim)?;
    let items = rows
        .into_iter()
        .map(|(key, bytes, flows)| TopItem {
            label: label(dim, &key, &labels),
            key,
            bytes,
            flows,
        })
        .collect();
    Ok((items, src.label()))
}
