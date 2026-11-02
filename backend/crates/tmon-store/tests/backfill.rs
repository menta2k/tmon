//! Late hostname binding for recent flows (names learned after the flow arrived).

mod common;

use common::*;
use tmon_core::{Categorizer, Direction, FlowRecord};
use tmon_store::FlowStore;
use tmon_store::duck::backfill::{BACKFILL_WINDOW_MS, backfill_hostnames};
use tmon_store::duck::repo::remote_hostnames;

const STREAMING: u16 = 2;
const WEB: u16 = 1;
const VIDEO_RULE: u32 = 7;

/// `*.googlevideo.com` → Streaming; everything else keeps its category.
struct Video;
impl Categorizer for Video {
    fn categorize(&self, f: &FlowRecord) -> (u16, Option<u32>) {
        match f.remote_host.as_deref() {
            Some(h) if h.ends_with(".googlevideo.com") => (STREAMING, Some(VIDEO_RULE)),
            _ => (f.category_id, f.rule_id),
        }
    }
}

const NOW: i64 = T0 + 30 * 60_000;
const CACHE: &str = "87.126.240.44";

fn seed() -> (tempfile::TempDir, std::sync::Arc<tmon_store::DuckStore>) {
    let (d, s) = open();
    let flows = vec![
        // Recent, unnamed → back-filled and moved to Streaming.
        rec(
            NOW - 60_000,
            "192.168.88.10",
            CACHE,
            443,
            5_000,
            WEB,
            Direction::Down,
        ),
        rec(
            NOW - 120_000,
            "192.168.88.11",
            CACHE,
            443,
            3_000,
            WEB,
            Direction::Down,
        ),
        // Too old for the back-fill window.
        rec(
            NOW - BACKFILL_WINDOW_MS - 60_000,
            "192.168.88.10",
            CACHE,
            443,
            1_000,
            WEB,
            Direction::Down,
        ),
        // No known name.
        rec(
            NOW - 60_000,
            "192.168.88.10",
            "1.1.1.1",
            443,
            700,
            WEB,
            Direction::Down,
        ),
    ];
    s.append_batch(&flows).unwrap();
    s.with_writer(|c| {
        remote_hostnames::upsert(
            c,
            &[(CACHE.into(), "rr1.sn-abc.googlevideo.com".into())],
            NOW,
        )
    })
    .unwrap();
    (d, s)
}

fn sum(s: &tmon_store::DuckStore, sql: &str) -> i64 {
    count(
        s,
        &format!("SELECT coalesce(sum(bytes), 0)::BIGINT FROM {sql}"),
    )
}

#[test]
fn names_and_recategorizes_recent_flows() {
    let (_d, s) = seed();
    let r = backfill_hostnames(&s, NOW, &Video).unwrap();
    assert_eq!((r.named, r.recategorized), (2, 2));

    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows_hot WHERE remote_host = 'rr1.sn-abc.googlevideo.com'"
        ),
        2
    );
    assert_eq!(
        count(
            &s,
            &format!(
                "SELECT count(*) FROM flows_hot WHERE category_id = {STREAMING} AND rule_id = {VIDEO_RULE}"
            )
        ),
        2
    );
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows_hot WHERE remote_host IS NULL"
        ),
        2,
        "old and unknown flows untouched"
    );
}

#[test]
fn moves_bytes_between_categories_in_rollups() {
    let (_d, s) = seed();
    let before: Vec<i64> = [
        "rollup_category_1m",
        "rollup_category_1h",
        "rollup_host_1h",
        "rollup_geo_1h",
    ]
    .iter()
    .map(|t| sum(&s, t))
    .collect();
    backfill_hostnames(&s, NOW, &Video).unwrap();
    for (t, total) in [
        "rollup_category_1m",
        "rollup_category_1h",
        "rollup_host_1h",
        "rollup_geo_1h",
    ]
    .iter()
    .zip(before)
    {
        assert_eq!(sum(&s, t), total, "{t}: total bytes preserved");
        assert_eq!(
            sum(&s, &format!("{t} WHERE category_id = {STREAMING}")),
            8_000,
            "{t}: moved to Streaming"
        );
        assert_eq!(
            sum(&s, &format!("{t} WHERE category_id = {WEB}")),
            1_700,
            "{t}: remainder stays Web"
        );
    }
    assert_eq!(
        count(
            &s,
            &format!(
                "SELECT count(*) FROM rollup_category_1m WHERE category_id = {WEB} AND flows = 0"
            )
        ),
        0,
        "emptied rows removed"
    );
    assert_eq!(
        sum(&s, "rollup_port_1h"),
        9_700,
        "category-independent rollups untouched"
    );
    assert_eq!(
        sum(&s, &format!("rollup_rule_1h WHERE rule_id = {VIDEO_RULE}")),
        8_000,
        "rule rollup follows"
    );
    assert_eq!(
        sum(&s, "rollup_rule_1m"),
        9_700,
        "rule rollup total preserved"
    );
}

#[test]
fn second_run_is_a_no_op() {
    let (_d, s) = seed();
    backfill_hostnames(&s, NOW, &Video).unwrap();
    let r = backfill_hostnames(&s, NOW, &Video).unwrap();
    assert_eq!((r.named, r.recategorized), (0, 0));
    assert_eq!(
        sum(
            &s,
            &format!("rollup_category_1h WHERE category_id = {STREAMING}")
        ),
        8_000
    );
}

#[test]
fn naming_without_category_change_leaves_rollups_alone() {
    let (_d, s) = open();
    s.append_batch(&[rec(
        NOW - 60_000,
        "192.168.88.10",
        "140.82.121.4",
        443,
        900,
        WEB,
        Direction::Down,
    )])
    .unwrap();
    s.with_writer(|c| {
        remote_hostnames::upsert(c, &[("140.82.121.4".into(), "github.com".into())], NOW)
    })
    .unwrap();
    let r = backfill_hostnames(&s, NOW, &Video).unwrap();
    assert_eq!((r.named, r.recategorized), (1, 0));
    assert_eq!(
        sum(&s, &format!("rollup_category_1h WHERE category_id = {WEB}")),
        900
    );
}

#[test]
fn captured_names_never_replace_router_dns_cache_names() {
    let (_d, s) = open();
    let row = |ip: &str, h: &str| vec![(ip.to_string(), h.to_string())];
    s.with_writer(|c| {
        remote_hostnames::upsert(c, &row("1.1.1.1", "one.one.one.one"), NOW)?;
        remote_hostnames::upsert_from(
            c,
            &row("1.1.1.1", "forged.example"),
            remote_hostnames::SOURCE_CAPTURE,
            NOW + 1,
        )?;
        remote_hostnames::upsert_from(
            c,
            &row("2.2.2.2", "cap.example"),
            remote_hostnames::SOURCE_CAPTURE,
            NOW,
        )?;
        remote_hostnames::upsert_from(
            c,
            &row("2.2.2.2", "cap2.example"),
            remote_hostnames::SOURCE_CAPTURE,
            NOW + 1,
        )?;
        // The router may still refresh a captured name.
        remote_hostnames::upsert(c, &row("2.2.2.2", "router.example"), NOW + 2)
    })
    .unwrap();
    let mut all = s.with_writer(remote_hostnames::all).unwrap();
    all.sort();
    assert_eq!(
        all,
        vec![
            ("1.1.1.1".into(), "one.one.one.one".into()),
            ("2.2.2.2".into(), "router.example".into())
        ]
    );
}
