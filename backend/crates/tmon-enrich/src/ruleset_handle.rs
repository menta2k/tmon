//! Built-in rule seeding and the live `RuleSet` snapshot (FR-009/FR-010).

use std::collections::HashMap;
use std::net::IpAddr;
use tmon_core::rules::{RuleSet, load_builtin};
use tmon_store::duck::repo::{categories, rules};
use tmon_store::{DuckStore, StoreError, StoreResult};

use crate::Pipeline;

/// Upsert shipped categories/rules (origin builtin). User changes to `enabled`,
/// category names and colours survive restarts.
pub fn seed_builtin(store: &DuckStore) -> StoreResult<()> {
    let b = load_builtin().map_err(|e| StoreError::Task(e.to_string()))?;
    store.with_writer(|c| {
        let tx = c.unchecked_transaction()?;
        categories::seed_builtin(&tx, &b.categories)?;
        rules::seed_builtin(&tx, &b.rules)?;
        tx.commit()?;
        Ok(())
    })
}

/// MAC → IPs from `local_host` (for MAC host specs).
pub fn mac_map(store: &DuckStore) -> StoreResult<HashMap<String, Vec<IpAddr>>> {
    let conn = store.conn()?;
    let mut stmt = conn.prepare("SELECT lower(mac), ip FROM local_host WHERE mac IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out: HashMap<String, Vec<IpAddr>> = HashMap::new();
    for row in rows {
        let (mac, ip) = row?;
        if let Ok(ip) = ip.parse() {
            out.entry(mac).or_default().push(ip);
        }
    }
    Ok(out)
}

/// Compile the enabled rules from the store and publish them to the pipeline.
pub fn reload(store: &DuckStore, pipeline: &Pipeline) -> StoreResult<usize> {
    let all = rules::list(&store.conn()?)?;
    let set = RuleSet::compile(&all, &mac_map(store)?);
    let n = set.len();
    pipeline.set_rules(set);
    tracing::debug!(rules = n, "rule set reloaded");
    Ok(n)
}

/// Seed then load (startup).
pub fn init(store: &DuckStore, pipeline: &Pipeline) -> StoreResult<usize> {
    seed_builtin(store)?;
    reload(store, pipeline)
}
