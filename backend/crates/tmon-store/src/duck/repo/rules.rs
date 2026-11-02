//! `rule` table (data-model.md §5, FR-010/FR-012).

use duckdb::{Connection, OptionalExt, params};
use serde::Serialize;
use std::collections::HashMap;
use tmon_core::ValidationErrors;
use tmon_core::rules::{Origin, Rule, RuleMatch, USER_PRIORITY_BASE};

use crate::{StoreError, StoreResult};

/// First id handed to user rules (built-ins use < 10 000).
pub const USER_ID_BASE: u32 = 10_000;
/// Priority gap between consecutive user rules.
pub const PRIORITY_STEP: i64 = 10;

/// Rule plus usage statistics (API view).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleView {
    #[serde(flatten)]
    pub rule: Rule,
    pub hits_24h: u64,
}

/// Fields of a new/updated user rule.
#[derive(Debug, Clone)]
pub struct RuleInput {
    pub name: String,
    pub category_id: u16,
    pub enabled: bool,
    pub match_: RuleMatch,
}

fn parse_row(r: &duckdb::Row<'_>) -> duckdb::Result<(Rule, String)> {
    let origin: String = r.get(4)?;
    let rule = Rule {
        id: r.get(0)?,
        category_id: r.get(1)?,
        priority: r.get(2)?,
        enabled: r.get(3)?,
        origin: if origin == "builtin" {
            Origin::Builtin
        } else {
            Origin::User
        },
        name: r.get(5)?,
        match_: RuleMatch::default(),
    };
    Ok((rule, r.get(6)?))
}

fn decode(rows: Vec<(Rule, String)>) -> StoreResult<Vec<Rule>> {
    rows.into_iter()
        .map(|(rule, json)| {
            let v: serde_json::Value = serde_json::from_str(&json)
                .map_err(|e| StoreError::Task(format!("rule {}: {e}", rule.id)))?;
            Ok(Rule {
                match_: RuleMatch::from_json(&v)?,
                ..rule
            })
        })
        .collect()
}

const COLS: &str = "id, category_id, priority, enabled, origin, name, match_json";

/// All rules, highest priority first.
pub fn list(conn: &Connection) -> StoreResult<Vec<Rule>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM rule ORDER BY priority DESC, id"
    ))?;
    let rows = stmt
        .query_map([], parse_row)?
        .collect::<Result<Vec<_>, _>>()?;
    decode(rows)
}

pub fn get(conn: &Connection, id: u32) -> StoreResult<Rule> {
    let row = conn
        .query_row(
            &format!("SELECT {COLS} FROM rule WHERE id = ?"),
            params![id],
            parse_row,
        )
        .optional()?;
    decode(row.into_iter().collect())?
        .pop()
        .ok_or(StoreError::NotFound)
}

fn match_json(m: &RuleMatch) -> StoreResult<String> {
    serde_json::to_string(m).map_err(|e| StoreError::Task(e.to_string()))
}

fn ensure_category(conn: &Connection, id: u16) -> StoreResult<()> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM category WHERE id = ?",
        params![id],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(StoreError::Validation(ValidationErrors::single(
            "category_id",
            "unknown category",
        )));
    }
    Ok(())
}

fn insert(conn: &Connection, r: &Rule) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO rule (id, category_id, priority, enabled, origin, name, match_json) VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![r.id, r.category_id, r.priority, r.enabled, r.origin.as_str(), r.name, match_json(&r.match_)?],
    )?;
    Ok(())
}

/// Create a user rule placed above all existing user rules.
pub fn create(conn: &Connection, input: RuleInput) -> StoreResult<Rule> {
    ensure_category(conn, input.category_id)?;
    let (id, top): (u32, i64) = conn.query_row(
        "SELECT CAST(greatest(coalesce(max(id), 0), ?) + 1 AS UINTEGER), \
         coalesce(max(priority) FILTER (WHERE origin = 'user'), ?) FROM rule",
        params![USER_ID_BASE - 1, USER_PRIORITY_BASE],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let rule = Rule {
        id,
        category_id: input.category_id,
        priority: top + PRIORITY_STEP,
        enabled: input.enabled,
        origin: Origin::User,
        name: input.name.trim().to_string(),
        match_: input.match_,
    };
    rule.validate()?;
    insert(conn, &rule)?;
    Ok(rule)
}

/// Update a rule. Built-in rules may only be enabled/disabled.
pub fn update(conn: &Connection, id: u32, f: impl FnOnce(&Rule) -> Rule) -> StoreResult<Rule> {
    let cur = get(conn, id)?;
    let next = Rule {
        id: cur.id,
        origin: cur.origin,
        priority: cur.priority,
        ..f(&cur)
    };
    if cur.origin == Origin::Builtin
        && (next.match_ != cur.match_
            || next.category_id != cur.category_id
            || next.name != cur.name)
    {
        return Err(StoreError::Conflict(
            "built-in rules can only be enabled or disabled".into(),
        ));
    }
    next.validate()?;
    ensure_category(conn, next.category_id)?;
    conn.execute(
        "UPDATE rule SET category_id = ?, enabled = ?, name = ?, match_json = ? WHERE id = ?",
        params![
            next.category_id,
            next.enabled,
            next.name,
            match_json(&next.match_)?,
            id
        ],
    )?;
    Ok(next)
}

pub fn delete(conn: &Connection, id: u32) -> StoreResult<()> {
    if get(conn, id)?.origin == Origin::Builtin {
        return Err(StoreError::Conflict(
            "built-in rules cannot be deleted; disable them instead".into(),
        ));
    }
    conn.execute("DELETE FROM rule WHERE id = ?", params![id])?;
    Ok(())
}

/// Assign priorities `BASE + (n - i) * STEP` to the given user rules (first = highest).
pub fn reorder(conn: &Connection, ids: &[u32]) -> StoreResult<()> {
    let users: Vec<u32> = list(conn)?
        .into_iter()
        .filter(|r| r.origin == Origin::User)
        .map(|r| r.id)
        .collect();
    let mut sorted_in = ids.to_vec();
    sorted_in.sort_unstable();
    sorted_in.dedup();
    let mut sorted_users = users.clone();
    sorted_users.sort_unstable();
    if sorted_in.len() != ids.len() || sorted_in != sorted_users {
        return Err(StoreError::Validation(ValidationErrors::single(
            "ids",
            "must list every user rule exactly once",
        )));
    }
    let n = ids.len() as i64;
    let tx = conn.unchecked_transaction()?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE rule SET priority = ? WHERE id = ?",
            params![USER_PRIORITY_BASE + (n - i as i64) * PRIORITY_STEP, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Upsert built-in rules: definition follows the shipped file, `enabled` is preserved.
pub fn seed_builtin(conn: &Connection, rules: &[Rule]) -> StoreResult<()> {
    for r in rules {
        conn.execute(
            "INSERT INTO rule (id, category_id, priority, enabled, origin, name, match_json) VALUES (?, ?, ?, ?, 'builtin', ?, ?) \
             ON CONFLICT DO UPDATE SET category_id = EXCLUDED.category_id, priority = EXCLUDED.priority, \
             name = EXCLUDED.name, match_json = EXCLUDED.match_json",
            params![r.id, r.category_id, r.priority, r.enabled, r.name, match_json(&r.match_)?],
        )?;
    }
    Ok(())
}

/// Replace all rules (settings import).
pub fn replace_all(conn: &Connection, rules: &[Rule]) -> StoreResult<()> {
    conn.execute("DELETE FROM rule", [])?;
    rules.iter().try_for_each(|r| insert(conn, r))
}

/// Bytes-agnostic hit counts per rule over `[from, to)` from the 1-minute category rollups is not
/// possible (no rule_id there), so count raw flows.
pub fn hits(
    conn: &Connection,
    raw_dir: &std::path::Path,
    from_ms: i64,
    to_ms: i64,
) -> StoreResult<HashMap<u32, u64>> {
    let files = crate::duck::files::raw_files(raw_dir, from_ms, to_ms)?;
    let sql = format!(
        "SELECT rule_id, count(*)::UBIGINT FROM ({}) WHERE ts >= ? AND ts < ? AND rule_id IS NOT NULL GROUP BY rule_id",
        crate::duck::files::union_sql(&files)
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from_ms, to_ms], |r| {
        Ok((r.get::<_, u32>(0)?, r.get::<_, u64>(1)?))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
