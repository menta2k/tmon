//! Late application binding for recent flows (feature 003, FR-015).
//!
//! The classifier often answers after the first flow records of a
//! connection were stored. Every few seconds, recent `flows_hot` rows of the
//! classified connections get their application, are re-categorized, and
//! their rollup contributions move between categories (same mechanics as
//! [`super::backfill`]).

use duckdb::{Connection, params};
use std::collections::HashMap;
use std::sync::Arc;
use tmon_core::app::AppInfo;
use tmon_core::naming::{ConnKey, NameSource};
use tmon_core::{Categorizer, FlowRecord};

use super::DuckStore;
use super::backfill::{BACKFILL_WINDOW_MS, Backfill, move_categories};
use super::recategorize::build_map;
use super::schema::FLOW_COLUMNS;
use crate::StoreResult;

/// Most classifications applied per pass.
pub const MAX_PER_PASS: usize = 10_000;

const TEMP_TABLES: &str = "DROP TABLE IF EXISTS bf_apps; DROP TABLE IF EXISTS bf_app_rows; \
     DROP TABLE IF EXISTS bf_moved; DROP TABLE IF EXISTS recat_map;";

/// Apply `items` (at most [`MAX_PER_PASS`]) to app-less raw flows of the
/// same connection from the last [`BACKFILL_WINDOW_MS`].
pub fn backfill_apps(
    store: &DuckStore,
    now_ms: i64,
    cat: &dyn Categorizer,
    items: &[(ConnKey, Arc<AppInfo>)],
) -> StoreResult<Backfill> {
    let items = &items[..items.len().min(MAX_PER_PASS)];
    if items.is_empty() {
        return Ok(Backfill::default());
    }
    store.with_writer(|c| {
        let result = run(c, now_ms - BACKFILL_WINDOW_MS, cat, items);
        c.execute_batch(TEMP_TABLES)?;
        result
    })
}

fn stage(c: &Connection, items: &[(ConnKey, Arc<AppInfo>)]) -> StoreResult<()> {
    c.execute_batch(
        "CREATE OR REPLACE TEMP TABLE bf_apps (lan_ip VARCHAR, lan_port USMALLINT, \
         remote_ip VARCHAR, remote_port USMALLINT, proto UTINYINT, app VARCHAR, \
         conf BOOLEAN, host VARCHAR)",
    )?;
    let mut app = c.appender("bf_apps")?;
    for (k, i) in items {
        app.append_row(params![
            k.lan_ip.to_string(),
            k.lan_port,
            k.remote_ip.to_string(),
            k.remote_port,
            k.proto,
            i.app.as_ref(),
            i.confident,
            i.host.as_deref(),
        ])?;
    }
    app.flush()?;
    Ok(())
}

fn run(
    c: &Connection,
    since_ms: i64,
    cat: &dyn Categorizer,
    items: &[(ConnKey, Arc<AppInfo>)],
) -> StoreResult<Backfill> {
    stage(c, items)?;
    c.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE bf_app_rows AS \
         SELECT f.rowid AS rn, a.app, a.conf, a.host FROM flows_hot f JOIN bf_apps a \
           ON f.local_ip = a.lan_ip AND f.local_port = a.lan_port AND f.remote_ip = a.remote_ip \
          AND f.remote_port = a.remote_port AND f.proto = a.proto \
         WHERE f.app IS NULL AND f.ts >= {since_ms} \
         QUALIFY row_number() OVER (PARTITION BY f.rowid ORDER BY a.conf DESC) = 1"
    ))?;
    let named: i64 = c.query_row("SELECT count(*) FROM bf_app_rows", [], |r| r.get(0))?;
    if named == 0 {
        return Ok(Backfill::default());
    }
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(&format!(
        "UPDATE flows_hot SET app = b.app, app_confident = b.conf, \
           remote_host = coalesce(flows_hot.remote_host, b.host), \
           remote_host_source = CASE WHEN flows_hot.remote_host IS NULL AND b.host IS NOT NULL \
             THEN {dpi}::UTINYINT ELSE flows_hot.remote_host_source END \
         FROM bf_app_rows b WHERE flows_hot.rowid = b.rn",
        dpi = NameSource::Dpi.code()
    ))?;
    let with_cats = WithAppCategory::new(cat, items);
    let changed = build_map(
        &tx,
        &format!(
            "SELECT rowid, {FLOW_COLUMNS} FROM flows_hot WHERE rowid IN (SELECT rn FROM bf_app_rows)"
        ),
        &with_cats,
    )?;
    if changed > 0 {
        move_categories(&tx)?;
    }
    tx.commit()?;
    Ok(Backfill {
        named: usize::try_from(named).unwrap_or(usize::MAX),
        recategorized: changed,
    })
}

/// The classifier category is not stored; supply it from the batch so
/// `app_category` rules match during back-fill too.
struct WithAppCategory<'a> {
    inner: &'a dyn Categorizer,
    cats: HashMap<ConnKey, Arc<str>>,
}

impl<'a> WithAppCategory<'a> {
    fn new(inner: &'a dyn Categorizer, items: &[(ConnKey, Arc<AppInfo>)]) -> Self {
        let cats = items
            .iter()
            .filter(|(_, i)| !i.category.is_empty())
            .map(|(k, i)| (*k, Arc::clone(&i.category)))
            .collect();
        Self { inner, cats }
    }
}

impl Categorizer for WithAppCategory<'_> {
    fn categorize(&self, flow: &FlowRecord) -> (u16, Option<u32>) {
        match self.cats.get(&ConnKey::from_record(flow)) {
            Some(c) if flow.app_category.is_none() => {
                let f = FlowRecord {
                    app_category: Some(Arc::clone(c)),
                    ..flow.clone()
                };
                self.inner.categorize(&f)
            }
            _ => self.inner.categorize(flow),
        }
    }
}
