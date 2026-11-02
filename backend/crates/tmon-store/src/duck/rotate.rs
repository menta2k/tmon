//! Hourly rotation of `flows_hot` into Parquet + `rollup_remote_1h`.

use duckdb::Connection;
use std::path::Path;

use super::files::{HOUR_MS, hour_start, next_path};
use super::schema::FLOW_COLUMNS;
use crate::StoreResult;

/// Remote endpoints kept per local host and hour in `rollup_remote_1h`.
pub const TOP_REMOTES_PER_HOST: usize = 200;

/// Rotate every complete hour older than `now_ms`'s hour. Returns files written.
pub fn rotate(conn: &Connection, raw_dir: &Path, now_ms: i64) -> StoreResult<usize> {
    let cutoff = hour_start(now_ms);
    let hours: Vec<i64> = conn
        .prepare("SELECT DISTINCT ts - (ts % 3600000) AS h FROM flows_hot WHERE ts < ? ORDER BY h")?
        .query_map([cutoff], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for h in &hours {
        rotate_hour(conn, raw_dir, *h)?;
    }
    Ok(hours.len())
}

fn sql_path(p: &Path) -> String {
    p.display().to_string().replace('\'', "''")
}

/// Write one hour to Parquet (temp file + rename), then drop it from the hot table.
pub fn rotate_hour(conn: &Connection, raw_dir: &Path, hour_ms: i64) -> StoreResult<()> {
    let end = hour_ms + HOUR_MS;
    let path = next_path(raw_dir, hour_ms);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("parquet.tmp");
    conn.execute_batch(&format!(
        "COPY (SELECT {FLOW_COLUMNS} FROM flows_hot WHERE ts >= {hour_ms} AND ts < {end} ORDER BY ts) \
         TO '{}' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 131072);",
        sql_path(&tmp)
    ))?;
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(&remote_rollup_sql(
        &format!("(SELECT * FROM flows_hot WHERE ts >= {hour_ms} AND ts < {end})"),
        hour_ms,
    ))?;
    std::fs::rename(&tmp, &path)?;
    tx.execute(
        "DELETE FROM flows_hot WHERE ts >= ? AND ts < ?",
        [hour_ms, end],
    )?;
    tx.commit()?;
    Ok(())
}

/// Upsert top remotes per local host for one hour from `source`.
pub fn remote_rollup_sql(source: &str, hour_ms: i64) -> String {
    format!(
        "INSERT INTO rollup_remote_1h \
         SELECT {hour_ms}, exporter_id, direction, local_ip, remote_ip, remote_host, country, asn, \
                as_name, bytes, packets, flows FROM ( \
           SELECT exporter_id, direction, local_ip, remote_ip, any_value(remote_host) AS remote_host, \
                  any_value(country) AS country, any_value(asn) AS asn, any_value(as_name) AS as_name, \
                  sum(bytes) AS bytes, sum(packets) AS packets, count(*) AS flows, \
                  row_number() OVER (PARTITION BY local_ip ORDER BY sum(bytes) DESC) AS rn \
           FROM {source} GROUP BY exporter_id, direction, local_ip, remote_ip) \
         WHERE rn <= {TOP_REMOTES_PER_HOST} \
         ON CONFLICT DO UPDATE SET bytes = bytes + EXCLUDED.bytes, packets = packets + EXCLUDED.packets, \
                                   flows = flows + EXCLUDED.flows;"
    )
}
