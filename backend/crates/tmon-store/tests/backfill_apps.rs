//! Late application binding for recent flows (feature 003, T032).

mod common;

use common::*;
use std::sync::Arc;
use tmon_core::app::AppInfo;
use tmon_core::naming::{ConnKey, NameSource};
use tmon_core::{Categorizer, Direction, FlowRecord};
use tmon_store::FlowStore;
use tmon_store::duck::backfill::BACKFILL_WINDOW_MS;
use tmon_store::duck::backfill_apps::{MAX_PER_PASS, backfill_apps};

const WEB: u16 = 1;
const FILE_SHARING: u16 = 15;
const VPN: u16 = 8;
const NOW: i64 = T0 + 30 * 60_000;
const PEER: &str = "203.0.113.5";

/// `app = BitTorrent` → File Sharing; `app_category = VPN` → VPN.
struct Apps;
impl Categorizer for Apps {
    fn categorize(&self, f: &FlowRecord) -> (u16, Option<u32>) {
        match (f.app.as_deref(), f.app_category.as_deref()) {
            (Some("BitTorrent"), _) => (FILE_SHARING, Some(1)),
            (_, Some("VPN")) => (VPN, Some(2)),
            _ => (f.category_id, f.rule_id),
        }
    }
}

fn key(f: &FlowRecord) -> ConnKey {
    ConnKey::from_record(f)
}

fn info(app: &str, cat: &str, confident: bool, host: Option<&str>) -> Arc<AppInfo> {
    Arc::new(AppInfo::new(app, cat, confident, host, true).unwrap())
}

fn flows() -> Vec<FlowRecord> {
    vec![
        rec(
            NOW - 60_000,
            "192.168.88.10",
            PEER,
            6881,
            5_000,
            WEB,
            Direction::Up,
        ),
        rec(
            NOW - 120_000,
            "192.168.88.10",
            PEER,
            6881,
            3_000,
            WEB,
            Direction::Up,
        ),
        rec(
            NOW - BACKFILL_WINDOW_MS - 60_000,
            "192.168.88.10",
            PEER,
            6881,
            1_000,
            WEB,
            Direction::Up,
        ),
        rec(
            NOW - 60_000,
            "192.168.88.11",
            PEER,
            6881,
            700,
            WEB,
            Direction::Up,
        ),
        rec(
            NOW - 60_000,
            "192.168.88.12",
            PEER,
            51820,
            900,
            WEB,
            Direction::Up,
        ),
    ]
}

#[test]
fn recent_matching_rows_get_the_app_and_move_category() {
    let (_d, s) = open();
    let f = flows();
    let already = f[3].with_app(&info("Steam", "Game", true, None));
    s.append_batch(&[
        f[0].clone(),
        f[1].clone(),
        f[2].clone(),
        already,
        f[4].clone(),
    ])
    .unwrap();
    let items = vec![
        (
            key(&f[0]),
            info("BitTorrent", "Download", true, Some("tracker.example.org")),
        ),
        (key(&f[3]), info("BitTorrent", "Download", true, None)),
        (key(&f[4]), info("WireGuard", "VPN", false, None)),
    ];
    let r = backfill_apps(&s, NOW, &Apps, &items).unwrap();
    assert_eq!(
        r.named, 3,
        "two BitTorrent rows in window + the WireGuard row"
    );
    assert_eq!(r.recategorized, 3);
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE app = 'BitTorrent' AND app_confident"
        ),
        2
    );
    assert_eq!(
        count(&s, "SELECT count(*) FROM flows WHERE app = 'Steam'"),
        1,
        "kept"
    );
    assert_eq!(
        count(&s, "SELECT count(*) FROM flows WHERE app IS NULL"),
        1,
        "too old"
    );
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE app = 'WireGuard' AND NOT app_confident AND category_id = 8"
        ),
        1,
        "app_category rule matched during back-fill"
    );
    assert_eq!(
        count(
            &s,
            &format!(
                "SELECT count(*) FROM flows WHERE remote_host = 'tracker.example.org' AND remote_host_source = {}",
                NameSource::Dpi.code()
            )
        ),
        2
    );
    assert_eq!(
        count(
            &s,
            "SELECT sum(bytes)::BIGINT FROM rollup_category_1h WHERE category_id = 15"
        ),
        8_000
    );
    assert_eq!(
        count(
            &s,
            "SELECT sum(bytes)::BIGINT FROM rollup_category_1h WHERE category_id = 1"
        ),
        1_000 + 700
    );
}

#[test]
fn existing_names_are_kept() {
    let (_d, s) = open();
    let f = flows()[0].with_remote_name("peer.example.com".into(), NameSource::Tls);
    s.append_batch(std::slice::from_ref(&f)).unwrap();
    let items = vec![(
        key(&f),
        info("BitTorrent", "Download", true, Some("other.example")),
    )];
    backfill_apps(&s, NOW, &Apps, &items).unwrap();
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE remote_host = 'peer.example.com' AND remote_host_source = 1"
        ),
        1
    );
}

#[test]
fn nothing_to_do() {
    let (_d, s) = open();
    assert_eq!(backfill_apps(&s, NOW, &Apps, &[]).unwrap().named, 0);
    let f = flows();
    s.append_batch(&f[..1]).unwrap();
    let other = ConnKey {
        lan_port: 1,
        ..key(&f[0])
    };
    let items = vec![(other, info("BitTorrent", "", true, None))];
    assert_eq!(backfill_apps(&s, NOW, &Apps, &items).unwrap().named, 0);
    assert_eq!(MAX_PER_PASS, 10_000);
}
