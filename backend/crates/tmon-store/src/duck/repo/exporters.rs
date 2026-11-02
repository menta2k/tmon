//! `exporter` table: persisted identity + settings of flow exporters.

use duckdb::{Connection, params};
use serde::Serialize;

use crate::StoreResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExporterRow {
    pub id: u16,
    pub address: String,
    pub domain_id: u32,
    pub protocol: String,
    pub name: Option<String>,
    pub sampling_override: Option<u32>,
    pub count_direction: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

pub fn list(conn: &Connection) -> StoreResult<Vec<ExporterRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, address, domain_id, protocol, name, sampling_override, count_direction, first_seen, last_seen \
         FROM exporter ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ExporterRow {
            id: r.get(0)?,
            address: r.get(1)?,
            domain_id: r.get(2)?,
            protocol: r.get(3)?,
            name: r.get(4)?,
            sampling_override: r.get(5)?,
            count_direction: r.get(6)?,
            first_seen: r.get(7)?,
            last_seen: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Insert or update an exporter row (identity is `id`).
pub fn upsert(conn: &Connection, e: &ExporterRow) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO exporter VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET \
         protocol = EXCLUDED.protocol, name = EXCLUDED.name, sampling_override = EXCLUDED.sampling_override, \
         count_direction = EXCLUDED.count_direction, last_seen = greatest(last_seen, EXCLUDED.last_seen)",
        params![e.id, e.address, e.domain_id, e.protocol, e.name, e.sampling_override, e.count_direction, e.first_seen, e.last_seen],
    )?;
    Ok(())
}
