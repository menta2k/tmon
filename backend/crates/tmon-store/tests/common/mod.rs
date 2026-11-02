#![allow(dead_code)]
use std::sync::Arc;
use tmon_core::{Direction, FlowRecord, RawFlow};
use tmon_store::{DuckStore, StoreOptions};

pub const HOUR: i64 = 3_600_000;
/// 2026-01-01T00:00:00Z
pub const T0: i64 = 1_767_225_600_000;

pub fn open() -> (tempfile::TempDir, Arc<DuckStore>) {
    let dir = tempfile::tempdir().unwrap();
    let store = DuckStore::open(dir.path(), &StoreOptions::default()).unwrap();
    (dir, Arc::new(store))
}

pub fn rec(
    ts: i64,
    local: &str,
    remote: &str,
    rport: u16,
    bytes: u64,
    cat: u16,
    dir: Direction,
) -> FlowRecord {
    let raw = RawFlow {
        ts_ms: ts,
        ts_start_ms: ts - 1000,
        exporter_id: 1,
        in_if: 1,
        out_if: 2,
        src_ip: local.parse().unwrap(),
        dst_ip: remote.parse().unwrap(),
        src_port: 40000,
        dst_port: rport,
        proto: 6,
        tcp_flags: 0,
        bytes,
        packets: 1,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    };
    FlowRecord::from_raw(&raw, dir, true)
        .with_category(cat, Some(1))
        .with_geo(Some("DE".into()), Some(3320), Some("DTAG".into()))
}

pub fn count(store: &DuckStore, sql: &str) -> i64 {
    store
        .conn()
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}
