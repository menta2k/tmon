//! `local_network` table (FR-007).

use duckdb::{Connection, params};
use serde::{Deserialize, Serialize};
use tmon_core::{Cidr, LocalNetworks, ValidationErrors};

use crate::StoreResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkRow {
    pub cidr: String,
    #[serde(default)]
    pub label: String,
}

pub fn list(conn: &Connection) -> StoreResult<Vec<NetworkRow>> {
    let mut stmt = conn.prepare("SELECT cidr, label FROM local_network ORDER BY cidr")?;
    let rows = stmt.query_map([], |r| {
        Ok(NetworkRow {
            cidr: r.get(0)?,
            label: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Validate rows (`cidr` must parse; label ≤ 80 chars) into a `LocalNetworks`.
pub fn validate(rows: &[NetworkRow]) -> Result<LocalNetworks, ValidationErrors> {
    let mut errors = ValidationErrors::new();
    let mut nets = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        match r.cidr.parse::<Cidr>() {
            Ok(c) => nets.push(c),
            Err(_) => errors.add(format!("[{i}].cidr"), "invalid CIDR"),
        }
        if r.label.chars().count() > 80 {
            errors.add(format!("[{i}].label"), "must be at most 80 characters");
        }
    }
    errors.into_result(LocalNetworks::new(nets))
}

pub fn load(conn: &Connection) -> StoreResult<LocalNetworks> {
    Ok(validate(&list(conn)?)?)
}

/// Replace all rows atomically (canonical CIDR form is stored).
pub fn replace(conn: &Connection, rows: &[NetworkRow]) -> StoreResult<LocalNetworks> {
    let nets = validate(rows)?;
    let tx = conn.unchecked_transaction()?;
    write_rows(&tx, rows)?;
    tx.commit()?;
    Ok(nets)
}

/// Replace all rows without opening a transaction (caller owns it). Rows must be validated.
pub fn write_rows(conn: &Connection, rows: &[NetworkRow]) -> StoreResult<()> {
    validate(rows)?;
    conn.execute("DELETE FROM local_network", [])?;
    for r in rows {
        let canonical = r
            .cidr
            .parse::<Cidr>()
            .map(|c| c.to_string())
            .unwrap_or_else(|_| r.cidr.clone());
        conn.execute(
            "INSERT INTO local_network VALUES (?, ?) ON CONFLICT DO NOTHING",
            params![canonical, r.label],
        )?;
    }
    conn.execute(
        "INSERT INTO setting VALUES ('networks_seeded', 'true') ON CONFLICT DO NOTHING",
        [],
    )?;
    Ok(())
}
