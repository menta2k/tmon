//! Decode a `FLOW_COLUMNS` row back into a `FlowRecord`.

use std::net::IpAddr;
use std::sync::Arc;
use tmon_core::{Direction, FlowRecord, NameSource};

fn ip(s: String, idx: usize) -> duckdb::Result<IpAddr> {
    s.parse().map_err(|e| {
        duckdb::Error::FromSqlConversionFailure(idx, duckdb::types::Type::Text, Box::new(e))
    })
}

/// `offset` = index of the first flow column in the row.
pub fn record(r: &duckdb::Row<'_>, offset: usize) -> duckdb::Result<FlowRecord> {
    let g = |i: usize| i + offset;
    let dir: String = r.get(g(15))?;
    let direction = Direction::parse(&dir).ok_or_else(|| {
        duckdb::Error::FromSqlConversionFailure(
            g(15),
            duckdb::types::Type::Text,
            format!("bad direction {dir}").into(),
        )
    })?;
    let nat: Option<String> = r.get(g(16))?;
    let arc = |v: Option<String>| v.map(Arc::<str>::from);
    let rec = FlowRecord {
        ts_ms: r.get(g(0))?,
        ts_start_ms: r.get(g(1))?,
        exporter_id: r.get(g(2))?,
        in_if: r.get(g(3))?,
        out_if: r.get(g(4))?,
        ip_ver: r.get(g(5))?,
        local_ip: ip(r.get(g(6))?, g(6))?,
        remote_ip: ip(r.get(g(7))?, g(7))?,
        local_port: r.get(g(8))?,
        remote_port: r.get(g(9))?,
        proto: r.get(g(10))?,
        tcp_flags: r.get(g(11))?,
        bytes: r.get(g(12))?,
        packets: r.get(g(13))?,
        sampled: r.get(g(14))?,
        direction,
        nat_ip: nat.map(|n| ip(n, g(16))).transpose()?,
        nat_port: r.get(g(17))?,
        category_id: r.get(g(18))?,
        rule_id: r.get(g(19))?,
        country: arc(r.get(g(20))?),
        asn: r.get(g(21))?,
        as_name: arc(r.get(g(22))?),
        remote_host: arc(r.get(g(23))?),
        remote_host_source: r
            .get::<_, Option<u8>>(g(24))?
            .and_then(NameSource::from_code),
        remote_aliases: None,
        app: None,
        app_confident: false,
        app_category: None,
    };
    Ok(rec.with_stored_app(arc(r.get(g(25))?), r.get(g(26))?))
}
