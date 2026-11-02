mod common;

use common::*;
use std::time::Duration;
use tmon_core::Direction;
use tmon_store::FlowStore;
use tmon_store::duck::files::raw_files;

#[test]
fn appended_batch_is_visible_through_flows_view() {
    let (_d, store) = open();
    store
        .append_batch(&[rec(
            T0 + 10,
            "192.168.1.2",
            "1.1.1.1",
            443,
            100,
            1,
            Direction::Up,
        )])
        .unwrap();
    assert_eq!(count(&store, "SELECT count(*) FROM flows"), 1);
    let host: String = store
        .conn()
        .unwrap()
        .query_row("SELECT remote_ip FROM flows", [], |r| r.get(0))
        .unwrap();
    assert_eq!(host, "1.1.1.1");
}

#[test]
fn rotation_writes_parquet_and_truncates_hot() {
    let (dir, store) = open();
    store
        .append_batch(&[
            rec(
                T0 + 1000,
                "192.168.1.2",
                "1.1.1.1",
                443,
                100,
                1,
                Direction::Up,
            ),
            rec(
                T0 + 2000,
                "192.168.1.3",
                "1.1.1.1",
                443,
                200,
                1,
                Direction::Down,
            ),
            rec(
                T0 + HOUR + 5,
                "192.168.1.2",
                "1.1.1.1",
                443,
                50,
                1,
                Direction::Up,
            ),
        ])
        .unwrap();
    let written = store.rotate(T0 + HOUR + 10).unwrap();
    assert_eq!(written, 1);
    let p = dir.path().join("raw/dt=2026-01-01/00.parquet");
    assert!(p.exists(), "parquet partition written");
    assert_eq!(
        count(&store, "SELECT count(*) FROM flows_hot"),
        1,
        "current hour stays hot"
    );
    assert_eq!(
        count(&store, "SELECT count(*) FROM flows"),
        3,
        "view unions hot + parquet"
    );
    assert_eq!(raw_files(store.raw_dir(), T0, T0 + HOUR).unwrap().len(), 1);
    assert_eq!(count(&store, "SELECT count(*) FROM rollup_remote_1h"), 2);
    // late data for an already rotated hour goes to a second file
    store
        .append_batch(&[rec(
            T0 + 3000,
            "192.168.1.2",
            "1.1.1.1",
            443,
            1,
            1,
            Direction::Up,
        )])
        .unwrap();
    store.rotate(T0 + HOUR + 20).unwrap();
    assert!(dir.path().join("raw/dt=2026-01-01/00_1.parquet").exists());
    assert_eq!(count(&store, "SELECT count(*) FROM flows"), 4);
    let remote_bytes = count(
        &store,
        "SELECT sum(bytes)::BIGINT FROM rollup_remote_1h WHERE local_ip = '192.168.1.2'",
    );
    assert_eq!(remote_bytes, 101);
}

#[test]
fn rollups_sum_repeated_keys() {
    let (_d, store) = open();
    let r = rec(
        T0 + 5_000,
        "192.168.1.2",
        "1.1.1.1",
        443,
        100,
        3,
        Direction::Up,
    );
    store.append_batch(std::slice::from_ref(&r)).unwrap();
    store.append_batch(&[r.clone(), r]).unwrap();
    for t in [
        "rollup_category_1m",
        "rollup_category_1h",
        "rollup_host_1m",
        "rollup_geo_1h",
        "rollup_port_1m",
    ] {
        assert_eq!(
            count(&store, &format!("SELECT count(*) FROM {t}")),
            1,
            "{t} one row"
        );
        assert_eq!(
            count(&store, &format!("SELECT sum(bytes)::BIGINT FROM {t}")),
            300,
            "{t} bytes"
        );
        assert_eq!(
            count(&store, &format!("SELECT sum(flows)::BIGINT FROM {t}")),
            3,
            "{t} flows"
        );
    }
    assert_eq!(count(&store, "SELECT bucket FROM rollup_category_1m"), T0);
}

#[tokio::test]
async fn writer_actor_flushes() {
    let (_d, store) = open();
    let (w, _h) = tmon_store::duck::writer::spawn(store.clone(), Duration::from_secs(3600));
    let now = tmon_store::duck::now_ms();
    w.send(vec![rec(
        now,
        "10.0.0.1",
        "8.8.8.8",
        53,
        10,
        9,
        Direction::Up,
    )])
    .await
    .unwrap();
    w.flush().await.unwrap();
    assert_eq!(count(&store, "SELECT count(*) FROM flows"), 1);
}

#[test]
fn reopen_keeps_data_and_seeds_once() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = tmon_store::DuckStore::open(dir.path(), &Default::default()).unwrap();
        s.append_batch(&[rec(T0, "10.0.0.1", "8.8.8.8", 53, 10, 9, Direction::Up)])
            .unwrap();
    }
    let s = tmon_store::DuckStore::open(dir.path(), &Default::default()).unwrap();
    assert_eq!(count(&s, "SELECT count(*) FROM flows"), 1);
    assert_eq!(count(&s, "SELECT count(*) FROM local_network"), 5);
    assert_eq!(count(&s, "SELECT count(*) FROM category WHERE id = 0"), 1);
    assert_eq!(count(&s, "SELECT max(version) FROM schema_migrations"), 7);
}
