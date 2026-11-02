//! Databases created by an older version gain new tables on upgrade.

mod common;

use common::*;
use tmon_core::Direction;
use tmon_store::{DuckStore, FlowStore, StoreOptions};

#[test]
fn database_from_before_rule_rollups_is_upgraded() {
    let (dir, s) = open();
    // Simulate a database created before migration 5 (no rule rollups).
    s.with_writer(|c| {
        c.execute_batch(
            "DROP TABLE rollup_rule_1m; DROP TABLE rollup_rule_1h; \
             DELETE FROM schema_migrations WHERE version >= 5;",
        )?;
        Ok(())
    })
    .unwrap();
    drop(s);

    let s = DuckStore::open(dir.path(), &StoreOptions::default()).unwrap();
    s.append_batch(&[rec(
        T0,
        "192.168.88.10",
        "1.1.1.1",
        443,
        100,
        1,
        Direction::Down,
    )])
    .unwrap();
    assert_eq!(
        count(&s, "SELECT sum(bytes)::BIGINT FROM rollup_rule_1h"),
        100
    );
    let tables = count(
        &s,
        "SELECT count(*) FROM information_schema.tables WHERE table_name IN ('rollup_rule_1m', 'rollup_rule_1h')",
    );
    assert_eq!(tables, 2);
}

#[test]
fn migration_6_adds_nullable_remote_host_source() {
    let (_d, s) = open();
    assert_eq!(count(&s, "SELECT max(version) FROM schema_migrations"), 7);
    let (ty, nullable): (String, String) = s
        .conn()
        .unwrap()
        .query_row(
            "SELECT data_type, is_nullable FROM information_schema.columns \
             WHERE table_name = 'flows_hot' AND column_name = 'remote_host_source'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((ty.as_str(), nullable.as_str()), ("UTINYINT", "YES"));
}

#[test]
fn database_from_before_migration_6_is_upgraded() {
    let (dir, s) = open();
    s.with_writer(|c| {
        c.execute_batch(
            "ALTER TABLE flows_hot DROP COLUMN remote_host_source; \
             DELETE FROM schema_migrations WHERE version >= 6;",
        )?;
        Ok(())
    })
    .unwrap();
    drop(s);
    let s = DuckStore::open(dir.path(), &StoreOptions::default()).unwrap();
    let r = rec(T0, "192.168.88.10", "1.1.1.1", 443, 100, 1, Direction::Down)
        .with_remote_name("one.one.one.one".into(), tmon_core::NameSource::Tls);
    s.append_batch(&[r]).unwrap();
    assert_eq!(count(&s, "SELECT remote_host_source::BIGINT FROM flows"), 1);
}

/// Rewrite a rotated file without `remote_host_source`, like files written
/// before feature 002.
fn downgrade_file(s: &DuckStore, path: &std::path::Path) {
    let p = path.display().to_string();
    let tmp = format!("{p}.old");
    s.with_writer(|c| {
        c.execute_batch(&format!(
            "COPY (SELECT * EXCLUDE (remote_host_source) FROM read_parquet('{p}')) TO '{tmp}' (FORMAT parquet);"
        ))?;
        Ok(())
    })
    .unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

#[test]
fn old_parquet_files_read_remote_host_source_as_null() {
    let (dir, s) = open();
    let named = rec(
        T0 + 10,
        "192.168.88.10",
        "1.1.1.1",
        443,
        100,
        1,
        Direction::Down,
    )
    .with_remote_name("one.one.one.one".into(), tmon_core::NameSource::Quic);
    s.append_batch(&[named]).unwrap();
    s.rotate(T0 + HOUR + 10).unwrap();
    let path = dir.path().join("raw/dt=2026-01-01/00.parquet");
    assert_eq!(count(&s, "SELECT remote_host_source::BIGINT FROM flows"), 2);
    downgrade_file(&s, &path);
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE remote_host_source IS NULL"
        ),
        1
    );

    // Recategorize rewrites and rebuilds from the old-schema file.
    let changed = tmon_store::duck::recategorize::recategorize(
        &s,
        T0,
        T0 + HOUR,
        &tmon_core::FallbackCategorizer,
        &|_| {},
    )
    .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(count(&s, "SELECT category_id::BIGINT FROM flows"), 0);
    assert_eq!(
        count(
            &s,
            "SELECT sum(bytes)::BIGINT FROM rollup_category_1h WHERE category_id = 0"
        ),
        100
    );
}

fn column(s: &DuckStore, table: &str, name: &str) -> (String, String) {
    s.conn()
        .unwrap()
        .query_row(
            "SELECT data_type, is_nullable FROM information_schema.columns \
             WHERE table_name = ? AND column_name = ?",
            [table, name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

fn app_rec(ts: i64, app: &str, confident: bool) -> tmon_core::FlowRecord {
    let info = tmon_core::AppInfo::new(app, "VPN", confident, None, true).unwrap();
    rec(ts, "192.168.88.10", "1.1.1.1", 51820, 100, 1, Direction::Up).with_app(&info)
}

#[test]
fn migration_7_adds_nullable_app_columns() {
    let (_d, s) = open();
    assert_eq!(
        column(&s, "flows_hot", "app"),
        ("VARCHAR".into(), "YES".into())
    );
    assert_eq!(
        column(&s, "flows_hot", "app_confident"),
        ("BOOLEAN".into(), "YES".into())
    );
}

#[test]
fn app_fields_round_trip_and_none_is_null() {
    let (_d, s) = open();
    let plain = rec(T0, "192.168.88.10", "1.1.1.1", 443, 100, 1, Direction::Down);
    s.append_batch(&[app_rec(T0, "WireGuard", true), plain])
        .unwrap();
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE app = 'WireGuard' AND app_confident"
        ),
        1
    );
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE app IS NULL AND app_confident IS NULL"
        ),
        1
    );
}

#[test]
fn database_from_before_migration_7_is_upgraded() {
    let (dir, s) = open();
    s.with_writer(|c| {
        c.execute_batch(
            "ALTER TABLE flows_hot DROP COLUMN app; ALTER TABLE flows_hot DROP COLUMN app_confident; \
             DELETE FROM schema_migrations WHERE version >= 7;",
        )?;
        Ok(())
    })
    .unwrap();
    drop(s);
    let s = DuckStore::open(dir.path(), &StoreOptions::default()).unwrap();
    s.append_batch(&[app_rec(T0, "Steam", false)]).unwrap();
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE app = 'Steam' AND NOT app_confident"
        ),
        1
    );
}

#[test]
fn old_parquet_files_read_app_as_null() {
    let (dir, s) = open();
    s.append_batch(&[app_rec(T0 + 10, "BitTorrent", true)])
        .unwrap();
    s.rotate(T0 + HOUR + 10).unwrap();
    let path = dir.path().join("raw/dt=2026-01-01/00.parquet");
    let p = path.display().to_string();
    let tmp = format!("{p}.old");
    s.with_writer(|c| {
        c.execute_batch(&format!(
            "COPY (SELECT * EXCLUDE (app, app_confident, remote_host_source) FROM read_parquet('{p}')) \
             TO '{tmp}' (FORMAT parquet);"
        ))?;
        Ok(())
    })
    .unwrap();
    std::fs::rename(&tmp, &path).unwrap();
    assert_eq!(count(&s, "SELECT count(*) FROM flows WHERE app IS NULL"), 1);
    let changed = tmon_store::duck::recategorize::recategorize(
        &s,
        T0,
        T0 + HOUR,
        &tmon_core::FallbackCategorizer,
        &|_| {},
    )
    .unwrap();
    assert!(changed <= 1);
    assert_eq!(count(&s, "SELECT count(*) FROM flows WHERE app IS NULL"), 1);
}
