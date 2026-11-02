//! `local_host` table (data-model.md §6, FR-012/FR-013).

use duckdb::{Connection, OptionalExt, params};
use serde::Serialize;
use std::collections::HashMap;
use tmon_core::ValidationErrors;
use tmon_core::rules::normalize_mac;

use crate::{StoreError, StoreResult};

const NAME_MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalHost {
    pub ip: String,
    pub mac: Option<String>,
    pub friendly_name: Option<String>,
    pub dhcp_hostname: Option<String>,
    /// Display name: friendly ?? DHCP ?? IP.
    pub name: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

/// `Some(None)` clears a field, `None` keeps it.
#[derive(Debug, Clone, Default)]
pub struct HostPatch {
    pub friendly_name: Option<Option<String>>,
    pub mac: Option<Option<String>>,
}

const COLS: &str = "ip, mac, friendly_name, dhcp_hostname, first_seen, last_seen";

fn row(r: &duckdb::Row<'_>) -> duckdb::Result<LocalHost> {
    let ip: String = r.get(0)?;
    let friendly: Option<String> = r.get(2)?;
    let dhcp: Option<String> = r.get(3)?;
    Ok(LocalHost {
        name: friendly
            .clone()
            .or_else(|| dhcp.clone())
            .unwrap_or_else(|| ip.clone()),
        ip,
        mac: r.get(1)?,
        friendly_name: friendly,
        dhcp_hostname: dhcp,
        first_seen: r.get(4)?,
        last_seen: r.get(5)?,
    })
}

/// Upsert seen times for local IPs: `(ip, first_ts, last_ts)`.
pub fn touch(conn: &Connection, seen: &[(String, i64, i64)]) -> StoreResult<()> {
    let tx = conn.unchecked_transaction()?;
    for (ip, first, last) in seen {
        tx.execute(
            "INSERT INTO local_host (ip, first_seen, last_seen) VALUES (?, ?, ?) ON CONFLICT DO UPDATE SET \
             first_seen = least(local_host.first_seen, EXCLUDED.first_seen), last_seen = greatest(local_host.last_seen, EXCLUDED.last_seen)",
            params![ip, first, last],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn get(conn: &Connection, ip: &str) -> StoreResult<LocalHost> {
    conn.query_row(
        &format!("SELECT {COLS} FROM local_host WHERE ip = ?"),
        params![ip],
        row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

/// Paged list with optional case-insensitive substring search over ip/names/MAC.
pub fn list(
    conn: &Connection,
    q: Option<&str>,
    page: u64,
    limit: u64,
) -> StoreResult<(Vec<LocalHost>, u64)> {
    let pattern = format!(
        "%{}%",
        q.unwrap_or("")
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let filter = "WHERE lower(concat_ws(' ', ip, coalesce(mac, ''), coalesce(friendly_name, ''), coalesce(dhcp_hostname, ''))) LIKE ? ESCAPE '\\'";
    let total: u64 = conn.query_row(
        &format!("SELECT count(*)::UBIGINT FROM local_host {filter}"),
        params![pattern],
        |r| r.get(0),
    )?;
    let offset = page.saturating_sub(1).saturating_mul(limit);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM local_host {filter} ORDER BY last_seen DESC, ip LIMIT ? OFFSET ?"
    ))?;
    let rows = stmt
        .query_map(params![pattern, limit, offset], row)?
        .collect::<Result<_, _>>()?;
    Ok((rows, total))
}

fn validate(p: &HostPatch) -> Result<HostPatch, ValidationErrors> {
    let mut e = ValidationErrors::new();
    let friendly = p
        .friendly_name
        .clone()
        .map(|o| o.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    if let Some(Some(n)) = &friendly
        && n.chars().count() > NAME_MAX
    {
        e.add(
            "friendly_name",
            format!("must be at most {NAME_MAX} characters"),
        );
    }
    let mac = match &p.mac {
        Some(Some(m)) if !m.trim().is_empty() => match normalize_mac(m) {
            Some(n) => Some(Some(n)),
            None => {
                e.add("mac", "must be a MAC address like aa:bb:cc:dd:ee:ff");
                None
            }
        },
        Some(_) => Some(None),
        None => None,
    };
    e.into_result(HostPatch {
        friendly_name: friendly,
        mac,
    })
}

pub fn patch(conn: &Connection, ip: &str, p: &HostPatch) -> StoreResult<LocalHost> {
    let p = validate(p)?;
    let cur = get(conn, ip)?;
    let friendly = p.friendly_name.unwrap_or(cur.friendly_name);
    let mac = p.mac.unwrap_or(cur.mac);
    conn.execute(
        "UPDATE local_host SET friendly_name = ?, mac = ? WHERE ip = ?",
        params![friendly, mac, ip],
    )?;
    get(conn, ip)
}

/// Apply DHCP leases `(ip, mac, hostname)` from the router; creates unseen hosts.
pub fn apply_leases(
    conn: &Connection,
    leases: &[(String, Option<String>, Option<String>)],
    now_ms: i64,
) -> StoreResult<()> {
    let tx = conn.unchecked_transaction()?;
    for (ip, mac, host) in leases {
        let mac = mac.as_deref().and_then(normalize_mac);
        tx.execute(
            "INSERT INTO local_host (ip, mac, dhcp_hostname, first_seen, last_seen) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT DO UPDATE SET dhcp_hostname = EXCLUDED.dhcp_hostname, mac = coalesce(local_host.mac, EXCLUDED.mac)",
            params![ip, mac, host, now_ms, now_ms],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// ip → display name for every known host.
pub fn names(conn: &Connection) -> StoreResult<HashMap<String, String>> {
    let mut stmt =
        conn.prepare("SELECT ip, coalesce(friendly_name, dhcp_hostname, ip) FROM local_host")?;
    Ok(stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?)
}

/// Replace all hosts' user fields (settings import): `(ip, mac, friendly_name)`.
pub fn import(
    conn: &Connection,
    rows: &[(String, Option<String>, Option<String>)],
    now_ms: i64,
) -> StoreResult<()> {
    for (ip, mac, friendly) in rows {
        conn.execute(
            "INSERT INTO local_host (ip, mac, friendly_name, first_seen, last_seen) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT DO UPDATE SET mac = EXCLUDED.mac, friendly_name = EXCLUDED.friendly_name",
            params![ip, mac, friendly, now_ms, now_ms],
        )?;
    }
    Ok(())
}
