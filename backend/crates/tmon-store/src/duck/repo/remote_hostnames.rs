//! `remote_hostname` table: remote IP → hostname from the router DNS cache
//! (feature 001, FR-013) or captured DNS answers (feature 002, FR-012).

use duckdb::{Connection, params};

use crate::StoreResult;

pub const PURGE_AFTER_MS: i64 = 7 * 86_400_000;
/// Hard cap on rows; the least recently seen are dropped beyond it, so a
/// flood of forged DNS answers cannot grow the table without bound.
pub const MAX_ROWS: i64 = 200_000;
/// `source` of names polled from the router DNS cache.
pub const SOURCE_ROUTER: &str = "dns_cache";
/// `source` of names from captured DNS answers.
pub const SOURCE_CAPTURE: &str = "capture";

/// Upsert router DNS-cache `(ip, hostname)` pairs seen at `now_ms`, then purge stale rows.
pub fn upsert(conn: &Connection, entries: &[(String, String)], now_ms: i64) -> StoreResult<()> {
    upsert_from(conn, entries, SOURCE_ROUTER, now_ms)
}

/// Upsert pairs learned from `source`, then purge stale rows.
pub fn upsert_from(
    conn: &Connection,
    entries: &[(String, String)],
    source: &str,
    now_ms: i64,
) -> StoreResult<()> {
    let tx = conn.unchecked_transaction()?;
    for (ip, host) in entries {
        tx.execute(
            // Captured names never replace a router DNS-cache name (trusted
            // source); they only refresh rows of their own source.
            "INSERT INTO remote_hostname VALUES (?, ?, ?, ?) ON CONFLICT DO UPDATE SET hostname = EXCLUDED.hostname, \
             source = EXCLUDED.source, last_seen = EXCLUDED.last_seen \
             WHERE EXCLUDED.source = 'dns_cache' OR remote_hostname.source <> 'dns_cache'",
            params![ip, host, source, now_ms],
        )?;
    }
    tx.execute(
        "DELETE FROM remote_hostname WHERE last_seen < ?",
        params![now_ms - PURGE_AFTER_MS],
    )?;
    tx.execute(
        "DELETE FROM remote_hostname WHERE ip IN (SELECT ip FROM remote_hostname \
         ORDER BY last_seen DESC OFFSET ?)",
        params![MAX_ROWS],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn all(conn: &Connection) -> StoreResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT ip, hostname FROM remote_hostname")?;
    Ok(stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?)
}
