//! Seed a data dir with 30 days of rollups and 24 h of raw Parquet (quickstart V14):
//! `cargo run --release -p tmon --bin seed_rollups -- <data_dir> [flows_per_hour=10000] [days=30]`.
//! Synthetic flows are generated in SQL and folded through the real rollup/rotation code paths.
use std::path::PathBuf;

use anyhow::Context;
use tmon_store::duck::files::{HOUR_MS, hour_start};
use tmon_store::duck::ingest::rollup_sql;
use tmon_store::duck::rotate::{remote_rollup_sql, rotate};
use tmon_store::duck::{Connection, DuckStore, StoreOptions, now_ms, schema::FLOW_COLUMNS};

const DAY_MS: i64 = 24 * HOUR_MS;

/// One day of synthetic flows into the temp table `seed_stage` (same columns as `flows_hot`).
fn stage_sql(day_start: i64, n: i64) -> String {
    format!(
        "CREATE OR REPLACE TEMP TABLE seed_stage AS \
         WITH c AS (SELECT list(id ORDER BY id) AS ids FROM category), \
         g AS (SELECT i, (hash(i + {day_start}) % 1000000007)::BIGINT AS h FROM range({n}) t(i)) \
         SELECT {day_start} + i * {DAY_MS} // {n} AS ts, {day_start} + i * {DAY_MS} // {n} - 30000 AS ts_start, \
           1::USMALLINT AS exporter_id, 1::UINTEGER AS in_if, 2::UINTEGER AS out_if, 4::UTINYINT AS ip_ver, \
           '192.168.88.' || (2 + h % 30) AS local_ip, \
           '203.0.' || (112 + (h // 30) % 8) || '.' || ((h // 240) % 256) AS remote_ip, \
           (40000 + i % 20000)::USMALLINT AS local_port, \
           [443, 443, 443, 80, 53, 27015, 51820, 993, 123, 8443][1 + (h // 7) % 10]::USMALLINT AS remote_port, \
           (CASE WHEN (h // 7) % 10 IN (4, 5, 6, 8) THEN 17 ELSE 6 END)::UTINYINT AS proto, \
           27::UTINYINT AS tcp_flags, (500 + (h // 11) % 4000000)::UBIGINT AS bytes, \
           (1 + (h // 11) % 4000000 // 1200)::UBIGINT AS packets, false AS sampled, \
           CASE WHEN h % 4 = 0 THEN 'up' ELSE 'down' END AS direction, \
           NULL::VARCHAR AS nat_ip, NULL::USMALLINT AS nat_port, \
           (SELECT ids[1 + (h // 13) % len(ids)] FROM c)::USMALLINT AS category_id, NULL::UINTEGER AS rule_id, \
           ['US', 'DE', 'NL', 'BG', 'GB', 'FR', 'JP'][1 + (h // 17) % 7] AS country, \
           [15169, 16509, 13335, 32934, 8075, 20940, 2906][1 + (h // 17) % 7]::UINTEGER AS asn, \
           ['GOOGLE', 'AMAZON-02', 'CLOUDFLARENET', 'FACEBOOK', 'MICROSOFT', 'Akamai', 'NETFLIX'][1 + (h // 17) % 7] AS as_name, \
           NULL::VARCHAR AS remote_host \
         FROM g;"
    )
}

fn seed_day(
    conn: &Connection,
    day_start: i64,
    n: i64,
    raw: bool,
    now: i64,
) -> tmon_store::StoreResult<()> {
    conn.execute_batch(&stage_sql(day_start, n))?;
    conn.execute("DELETE FROM seed_stage WHERE ts > ?", [now])?;
    if raw {
        return conn
            .execute_batch(&format!(
                "INSERT INTO flows_hot SELECT {FLOW_COLUMNS} FROM seed_stage;"
            ))
            .map_err(Into::into);
    }
    let mut sql = format!("BEGIN; {}", rollup_sql("seed_stage"));
    for h in 0..24 {
        let hour = day_start + h * HOUR_MS;
        sql.push_str(&remote_rollup_sql(
            &format!(
                "(SELECT * FROM seed_stage WHERE ts >= {hour} AND ts < {})",
                hour + HOUR_MS
            ),
            hour,
        ));
    }
    sql.push_str("COMMIT;");
    conn.execute_batch(&sql).map_err(Into::into)
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .context("usage: seed_rollups <data_dir> [flows_per_hour] [days]")?,
    );
    let per_hour: i64 = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(10_000);
    let days: i64 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(30);
    anyhow::ensure!(
        per_hour > 0 && (1..=400).contains(&days),
        "flows_per_hour > 0, 1 ≤ days ≤ 400"
    );
    let store = DuckStore::open(&dir, &StoreOptions::default())?;
    // Categories are seeded at app start; the generator picks ids from that table.
    tmon_enrich::ruleset_handle::seed_builtin(&store)?;
    let now = now_ms();
    let today = hour_start(now) - 23 * HOUR_MS; // last 24 complete-ish hours are raw
    store.with_writer(|c| {
        c.execute_batch(&format!(
            "INSERT INTO exporter VALUES (1, '192.168.88.1', 0, 'ipfix', 'seed', NULL, 'both', {now}, {now}) ON CONFLICT DO NOTHING;"
        ))?;
        for d in (1..days).rev() {
            seed_day(c, today - d * DAY_MS, per_hour * 24, false, now)?;
            eprintln!("rollups: day -{d}");
        }
        seed_day(c, today, per_hour * 24, true, now)?;
        c.execute_batch(&rollup_sql("seed_stage"))?;
        let files = rotate(c, store.raw_dir(), now)?;
        eprintln!("raw: {files} hourly Parquet files");
        Ok(())
    })?;
    store.refresh_view()?;
    println!("seeded {days} days into {}", dir.display());
    Ok(())
}
