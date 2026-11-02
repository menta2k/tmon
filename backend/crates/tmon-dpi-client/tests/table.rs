//! AppTable (data-model.md §5).

mod common;

use common::key;
use std::sync::Arc;
use tmon_core::app::{AppInfo, ConnectionClassifier};
use tmon_dpi_client::AppTable;
use tmon_dpi_client::table::Insert;

fn info(app: &str, confident: bool) -> Arc<AppInfo> {
    Arc::new(AppInfo::new(app, "VPN", confident, None, false).unwrap())
}

#[test]
fn guess_does_not_replace_confident() {
    let t = AppTable::new(100, 1_000);
    assert_eq!(t.insert(key(1), info("WireGuard", true), 0), Insert::Stored);
    assert_eq!(t.insert(key(1), info("OpenVPN", false), 1), Insert::Kept);
    assert_eq!(t.insert(key(1), info("WireGuard", true), 2), Insert::Kept);
    assert_eq!(t.classify(&key(1), 3).unwrap().app.as_ref(), "WireGuard");
    assert_eq!(t.insert(key(1), info("Steam", true), 4), Insert::Stored);
}

#[test]
fn ttl_counts_from_last_use() {
    let t = AppTable::new(100, 1_000);
    t.insert(key(2), info("Steam", true), 0);
    assert!(t.get(&key(2), 900).is_some());
    assert!(t.get(&key(2), 1_800).is_some(), "refreshed at 900");
    assert!(t.get(&key(2), 2_801).is_none());
    assert!(t.is_empty());
    // An expired guess may be replaced by anything.
    t.insert(key(3), info("WireGuard", true), 0);
    assert_eq!(
        t.insert(key(3), info("OpenVPN", false), 5_000),
        Insert::Stored
    );
}

#[test]
fn capacity_is_bounded_and_evictions_counted() {
    let t = AppTable::new(32, 1_000_000);
    for p in 0..1_000u16 {
        t.insert(key(p), info("Steam", true), 0);
    }
    assert!(t.len() <= 32 + 16);
    assert!(t.evicted() >= 1_000 - 48);
}

#[test]
fn concurrent_writers() {
    let t = Arc::new(AppTable::new(100_000, 1_000_000));
    let threads: Vec<_> = (0..8u16)
        .map(|i| {
            let t = Arc::clone(&t);
            std::thread::spawn(move || {
                for p in 0..1_000u16 {
                    t.insert(key(i * 1_000 + p), info("Steam", true), 0);
                }
            })
        })
        .collect();
    threads.into_iter().for_each(|h| h.join().unwrap());
    assert_eq!(t.len(), 8_000);
}
