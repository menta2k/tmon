mod common;

use common::*;
use tmon_core::Direction;
use tmon_store::FlowStore;
use tmon_store::duck::retention::{Policy, disk_bytes, run};

const BIG: u64 = u64::MAX / 2;

fn seeded() -> (tempfile::TempDir, std::sync::Arc<tmon_store::DuckStore>) {
    let (dir, s) = open();
    let flows: Vec<_> = (0..4)
        .map(|h| {
            rec(
                T0 + h * HOUR + 60_000,
                "192.168.88.10",
                "1.1.1.1",
                443,
                1000,
                1,
                Direction::Down,
            )
        })
        .collect();
    s.append_batch(&flows).unwrap();
    assert_eq!(s.rotate(T0 + 4 * HOUR).unwrap(), 4);
    (dir, s)
}

fn policy(raw_ms: i64, m1: i64, h1: i64, cap: u64) -> Policy {
    Policy {
        raw_ms,
        rollup_1m_ms: m1,
        rollup_1h_ms: h1,
        cap_bytes: cap,
    }
}

#[test]
fn expired_raw_hours_are_deleted() {
    let (_d, s) = seeded();
    let r = run(
        &s,
        &policy(2 * HOUR + HOUR / 2, 100 * HOUR, 100 * HOUR, BIG),
        T0 + 4 * HOUR,
    )
    .unwrap();
    assert_eq!(r.expired_files, 1);
    assert_eq!(r.capped_files, 0);
    assert_eq!(r.oldest_raw, Some(T0 + HOUR));
    assert_eq!(count(&s, "SELECT count(*) FROM flows"), 3);
    assert!(!r.cap_exceeded());
}

#[test]
fn expired_rollup_rows_are_deleted() {
    let (_d, s) = seeded();
    assert_eq!(count(&s, "SELECT count(*) FROM rollup_category_1m"), 4);
    run(
        &s,
        &policy(100 * HOUR, 3 * HOUR, 100 * HOUR, BIG),
        T0 + 4 * HOUR,
    )
    .unwrap();
    assert_eq!(count(&s, "SELECT count(*) FROM rollup_category_1m"), 3);
    assert_eq!(count(&s, "SELECT count(*) FROM rollup_category_1h"), 4);
    run(
        &s,
        &policy(100 * HOUR, 3 * HOUR, 2 * HOUR, BIG),
        T0 + 4 * HOUR,
    )
    .unwrap();
    assert_eq!(count(&s, "SELECT count(*) FROM rollup_category_1h"), 2);
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM rollup_remote_1h WHERE bucket < 1767232800000"
        ),
        0
    );
}

#[test]
fn disk_cap_removes_oldest_raw_first_and_flags_it() {
    let (_d, s) = seeded();
    let used = disk_bytes(&s).unwrap();
    let r = run(
        &s,
        &policy(100 * HOUR, 100 * HOUR, 100 * HOUR, used - 1),
        T0 + 4 * HOUR,
    )
    .unwrap();
    assert!(r.capped_files >= 1);
    assert!(r.cap_exceeded());
    assert!(r.oldest_raw.unwrap() > T0, "oldest hour went first");
    assert_eq!(
        count(&s, "SELECT count(*) FROM flows") as usize,
        4 - r.capped_files
    );
}
