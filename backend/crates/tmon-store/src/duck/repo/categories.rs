//! `category` table (data-model.md §4, FR-011).

use duckdb::{Connection, OptionalExt, params};
use serde::Deserialize;
use tmon_core::category::{OTHER_ID, validate_fields};
use tmon_core::{Category, ValidationErrors};

use crate::{StoreError, StoreResult};

/// Partial update; `None` keeps the current value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryPatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
}

fn row(r: &duckdb::Row<'_>) -> duckdb::Result<Category> {
    Ok(Category {
        id: r.get(0)?,
        name: r.get(1)?,
        color: r.get(2)?,
        description: r.get(3)?,
        builtin: r.get(4)?,
    })
}

const COLS: &str = "id, name, color, description, builtin";

pub fn list(conn: &Connection) -> StoreResult<Vec<Category>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM category ORDER BY id"))?;
    Ok(stmt.query_map([], row)?.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: u16) -> StoreResult<Category> {
    conn.query_row(
        &format!("SELECT {COLS} FROM category WHERE id = ?"),
        params![id],
        row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

fn ensure_unique_name(conn: &Connection, name: &str, except: Option<u16>) -> StoreResult<()> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM category WHERE lower(name) = lower(?) AND id IS DISTINCT FROM ?",
        params![name.trim(), except],
        |r| r.get(0),
    )?;
    if n > 0 {
        return Err(StoreError::Validation(ValidationErrors::single(
            "name",
            "already exists",
        )));
    }
    Ok(())
}

pub fn create(
    conn: &Connection,
    name: &str,
    color: &str,
    description: &str,
) -> StoreResult<Category> {
    validate_fields(name, color, description)?;
    ensure_unique_name(conn, name, None)?;
    let id: u16 = conn.query_row(
        "SELECT CAST(greatest(coalesce(max(id), 0), 99) + 1 AS USMALLINT) FROM category",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO category (id, name, color, description, builtin) VALUES (?, ?, ?, ?, false)",
        params![id, name.trim(), color, description],
    )?;
    get(conn, id)
}

pub fn update(conn: &Connection, id: u16, patch: &CategoryPatch) -> StoreResult<Category> {
    let cur = get(conn, id)?;
    let next = Category {
        name: patch
            .name
            .clone()
            .map(|n| n.trim().to_string())
            .unwrap_or(cur.name),
        color: patch.color.clone().unwrap_or(cur.color),
        description: patch.description.clone().unwrap_or(cur.description),
        ..cur
    };
    validate_fields(&next.name, &next.color, &next.description)?;
    ensure_unique_name(conn, &next.name, Some(id))?;
    conn.execute(
        "UPDATE category SET name = ?, color = ?, description = ? WHERE id = ?",
        params![next.name, next.color, next.description, id],
    )?;
    Ok(next)
}

/// Delete a user category; built-ins and "Other" are protected, and so are categories still used by rules.
pub fn delete(conn: &Connection, id: u16) -> StoreResult<()> {
    let cur = get(conn, id)?;
    if cur.builtin || id == OTHER_ID {
        return Err(StoreError::Conflict(
            "built-in categories cannot be deleted".into(),
        ));
    }
    let used: i64 = conn.query_row(
        "SELECT count(*) FROM rule WHERE category_id = ?",
        params![id],
        |r| r.get(0),
    )?;
    if used > 0 {
        return Err(StoreError::Conflict(format!(
            "category is used by {used} rule(s)"
        )));
    }
    conn.execute("DELETE FROM category WHERE id = ?", params![id])?;
    Ok(())
}

/// Insert built-in categories that are missing; user renames/recolours are kept.
pub fn seed_builtin(conn: &Connection, cats: &[Category]) -> StoreResult<()> {
    for c in cats {
        conn.execute(
            "INSERT INTO category (id, name, color, description, builtin) VALUES (?, ?, ?, ?, true) ON CONFLICT DO NOTHING",
            params![c.id, c.name, c.color, c.description],
        )?;
    }
    Ok(())
}

/// Replace all categories (settings import).
pub fn replace_all(conn: &Connection, cats: &[Category]) -> StoreResult<()> {
    conn.execute("DELETE FROM category", [])?;
    for c in cats {
        conn.execute(
            "INSERT INTO category (id, name, color, description, builtin) VALUES (?, ?, ?, ?, ?)",
            params![c.id, c.name, c.color, c.description, c.builtin],
        )?;
    }
    Ok(())
}
