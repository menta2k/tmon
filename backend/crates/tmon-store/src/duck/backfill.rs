//! Late hostname binding for recent flows.
//!
//! Remote host names come from polling the router's DNS cache, so a connection
//! can be exported before its name is learned. After each DNS poll, recent
//! unnamed flows in `flows_hot` get their name, are re-categorized, and their
//! bytes are moved between categories in the category-dependent rollups.
//! (`rollup_remote_1h` is built from `flows_hot` at rotation, so it picks the
//! names up by itself.)

use duckdb::Connection;
use tmon_core::Categorizer;

use super::DuckStore;
use super::ingest::rollup_sql_for;
use super::recategorize::build_map;
use super::schema::{FLOW_COLUMNS, GRAINS};
use crate::StoreResult;

/// How far back unnamed flows are revisited (well beyond the DNS poll interval).
pub const BACKFILL_WINDOW_MS: i64 = 15 * 60_000;

/// Rollups keyed by category, with each key column and the expression the
/// ingest upsert uses for it (see `ingest::dims`).
const CATEGORY_ROLLUPS: [(&str, &[(&str, &str)]); 4] = [
    (
        "rollup_category",
        &[("category_id", "category_id"), ("proto", "proto")],
    ),
    (
        "rollup_host",
        &[("local_ip", "local_ip"), ("category_id", "category_id")],
    ),
    (
        "rollup_geo",
        &[
            ("country", "coalesce(country, '')"),
            ("asn", "coalesce(asn, 0)"),
            ("category_id", "category_id"),
        ],
    ),
    (
        "rollup_rule",
        &[
            ("rule_id", "coalesce(rule_id, 0)"),
            ("category_id", "category_id"),
        ],
    ),
];

const TEMP_TABLES: &str =
    "DROP TABLE IF EXISTS bf_named; DROP TABLE IF EXISTS bf_moved; DROP TABLE IF EXISTS recat_map;";

/// Outcome of one back-fill pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Backfill {
    /// Flows that received a host name.
    pub named: usize,
    /// Of those, flows whose category or rule changed.
    pub recategorized: usize,
}

/// Name and re-categorize unnamed `flows_hot` rows from the last
/// [`BACKFILL_WINDOW_MS`] whose remote IP is now in `remote_hostname`.
pub fn backfill_hostnames(
    store: &DuckStore,
    now_ms: i64,
    cat: &dyn Categorizer,
) -> StoreResult<Backfill> {
    store.with_writer(|c| {
        let result = run(c, now_ms - BACKFILL_WINDOW_MS, cat);
        c.execute_batch(TEMP_TABLES)?;
        result
    })
}

fn run(c: &Connection, since_ms: i64, cat: &dyn Categorizer) -> StoreResult<Backfill> {
    c.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE bf_named AS \
         SELECT f.rowid AS rn, rh.hostname, \
           CASE rh.source WHEN 'capture' THEN 4 ELSE 5 END::UTINYINT AS src \
         FROM flows_hot f JOIN remote_hostname rh ON rh.ip = f.remote_ip \
         WHERE f.remote_host IS NULL AND f.ts >= {since_ms}"
    ))?;
    let named: i64 = c.query_row("SELECT count(*) FROM bf_named", [], |r| r.get(0))?;
    if named == 0 {
        return Ok(Backfill::default());
    }
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(
        "UPDATE flows_hot SET remote_host = b.hostname, remote_host_source = b.src FROM bf_named b \
         WHERE flows_hot.rowid = b.rn",
    )?;
    let changed = build_map(
        &tx,
        &format!(
            "SELECT rowid, {FLOW_COLUMNS} FROM flows_hot WHERE rowid IN (SELECT rn FROM bf_named)"
        ),
        cat,
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

/// Apply `recat_map` to `flows_hot`, moving the affected rows' rollup contributions.
pub(super) fn move_categories(c: &Connection) -> StoreResult<()> {
    c.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE bf_moved AS SELECT rowid AS rn, {FLOW_COLUMNS} FROM flows_hot \
         WHERE rowid IN (SELECT m.rn FROM recat_map m JOIN flows_hot f ON f.rowid = m.rn \
           WHERE m.category_id IS DISTINCT FROM f.category_id OR m.rule_id IS DISTINCT FROM f.rule_id)"
    ))?;
    c.execute_batch(&subtract_sql("bf_moved"))?;
    c.execute_batch(
        "UPDATE flows_hot SET category_id = m.category_id, rule_id = m.rule_id \
         FROM recat_map m WHERE flows_hot.rowid = m.rn AND m.rn IN (SELECT rn FROM bf_moved)",
    )?;
    let families: Vec<&str> = CATEGORY_ROLLUPS.iter().map(|(f, _)| *f).collect();
    let source = format!(
        "(SELECT {FLOW_COLUMNS} FROM flows_hot WHERE rowid IN (SELECT rn FROM bf_moved)) AS bf_src"
    );
    c.execute_batch(&rollup_sql_for(&source, &families))?;
    Ok(())
}

/// Remove the contributions of `source` rows from the category rollups.
fn subtract_sql(source: &str) -> String {
    let mut sql = String::new();
    for (family, keys) in CATEGORY_ROLLUPS {
        let select: Vec<String> = keys
            .iter()
            .map(|(col, expr)| format!("{expr} AS {col}"))
            .collect();
        let join: Vec<String> = keys
            .iter()
            .map(|(col, _)| format!("r.{col} = d.{col}"))
            .collect();
        for (grain, ms) in GRAINS {
            let table = format!("{family}_{grain}");
            sql.push_str(&format!(
                "UPDATE {table} AS r SET bytes = r.bytes - d.bytes, packets = r.packets - d.packets, flows = r.flows - d.flows \
                 FROM (SELECT ts - (ts % {ms}) AS bucket, exporter_id, direction, {select}, \
                       sum(bytes) AS bytes, sum(packets) AS packets, count(*) AS flows FROM {source} GROUP BY ALL) d \
                 WHERE r.bucket = d.bucket AND r.exporter_id = d.exporter_id AND r.direction = d.direction AND {join};\n\
                 DELETE FROM {table} WHERE flows = 0;\n",
                select = select.join(", "),
                join = join.join(" AND "),
            ));
        }
    }
    sql
}
