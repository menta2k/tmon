//! Batch ingest: stage → hot table + incremental rollup upserts (one transaction).

use duckdb::{Appender, Connection, params};
use tmon_core::FlowRecord;

use super::schema::{FLOW_COLUMNS, GRAINS};
use crate::StoreResult;

const ASN_NAMES_SQL: &str = "INSERT INTO asn_name SELECT asn, any_value(as_name) FROM batch_stage \
    WHERE asn IS NOT NULL AND as_name IS NOT NULL GROUP BY asn ON CONFLICT DO UPDATE SET name = EXCLUDED.name;";

/// Remote ports outside the per-batch top-N are folded into port 0.
pub const TOP_PORTS: usize = 1000;

pub fn ingest(conn: &Connection, flows: &[FlowRecord]) -> StoreResult<()> {
    if flows.is_empty() {
        return Ok(());
    }
    conn.execute("DELETE FROM batch_stage", [])?;
    {
        let mut app = conn.appender("batch_stage")?;
        for f in flows {
            append(&mut app, f)?;
        }
        app.flush()?;
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(&format!(
        "INSERT INTO flows_hot BY NAME SELECT {FLOW_COLUMNS} FROM batch_stage;"
    ))?;
    tx.execute_batch(&rollup_sql("batch_stage"))?;
    tx.execute_batch(ASN_NAMES_SQL)?;
    tx.execute("DELETE FROM batch_stage", [])?;
    tx.commit()?;
    Ok(())
}

fn opt_ip(ip: Option<std::net::IpAddr>) -> Option<String> {
    ip.map(|i| i.to_string())
}

fn append(app: &mut Appender<'_>, f: &FlowRecord) -> StoreResult<()> {
    app.append_row(params![
        f.ts_ms,
        f.ts_start_ms,
        f.exporter_id,
        f.in_if,
        f.out_if,
        f.ip_ver,
        f.local_ip.to_string(),
        f.remote_ip.to_string(),
        f.local_port,
        f.remote_port,
        f.proto,
        f.tcp_flags,
        f.bytes,
        f.packets,
        f.sampled,
        f.direction.as_str(),
        opt_ip(f.nat_ip),
        f.nat_port,
        f.category_id,
        f.rule_id,
        f.country.as_deref(),
        f.asn,
        f.as_name.as_deref(),
        f.remote_host.as_deref(),
        f.remote_host_source.map(|s| s.code()),
        f.app.as_deref(),
        f.app.as_ref().map(|_| f.app_confident),
    ])?;
    Ok(())
}

/// Dimension expressions per rollup family (must match `schema::ROLLUPS` key order).
fn dims(family: &str, source: &str) -> String {
    match family {
        "rollup_category" => "category_id, proto".into(),
        "rollup_host" => "local_ip, category_id".into(),
        "rollup_geo" => "coalesce(country, ''), coalesce(asn, 0), category_id".into(),
        "rollup_rule" => "coalesce(rule_id, 0), category_id".into(),
        _ => format!(
            "proto, CASE WHEN remote_port IN (SELECT remote_port FROM {source} GROUP BY remote_port \
             ORDER BY sum(bytes) DESC LIMIT {TOP_PORTS}) THEN remote_port ELSE 0 END"
        ),
    }
}

/// Upsert all 1m/1h rollups from rows in `source`.
pub fn rollup_sql(source: &str) -> String {
    let all: Vec<&str> = super::schema::ROLLUPS
        .iter()
        .map(|(family, _, _)| *family)
        .collect();
    rollup_sql_for(source, &all)
}

/// Upsert the 1m/1h rollups of the given families from rows in `source`.
pub fn rollup_sql_for(source: &str, families: &[&str]) -> String {
    let mut sql = String::new();
    for family in families {
        for (grain, ms) in GRAINS {
            sql.push_str(&format!(
                "INSERT INTO {family}_{grain} SELECT ts - (ts % {ms}) AS bucket, exporter_id, direction, \
                 {d}, sum(bytes), sum(packets), count(*) FROM {source} GROUP BY ALL \
                 ON CONFLICT DO UPDATE SET bytes = bytes + EXCLUDED.bytes, \
                 packets = packets + EXCLUDED.packets, flows = flows + EXCLUDED.flows;\n",
                d = dims(family, source)
            ));
        }
    }
    sql
}
