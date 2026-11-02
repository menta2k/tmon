//! Breakdown by matched rule ("application") with confidence.

mod common;

use common::*;
use std::collections::HashMap;
use tmon_core::Direction;
use tmon_core::rules::load_builtin;
use tmon_store::FlowStore;
use tmon_store::duck::query::router::{Dim, Filter, Windows};
use tmon_store::duck::query::traffic::breakdown;
use tmon_store::duck::repo::{categories, rules};

const YOUTUBE: u32 = 1;
const WEB_CATCH_ALL: u32 = 900;

#[test]
fn breakdown_by_rule_labels_rules_and_reports_confidence() {
    let (_d, s) = open();
    let b = load_builtin().unwrap();
    s.with_writer(|c| {
        categories::seed_builtin(c, &b.categories)?;
        rules::seed_builtin(c, &b.rules)
    })
    .unwrap();
    let ts = T0 + 1_000;
    let with_rule = |remote: &str, bytes: u64, rule: Option<u32>| {
        rec(ts, "192.168.88.10", remote, 443, bytes, 1, Direction::Down).with_category(1, rule)
    };
    s.append_batch(&[
        with_rule("94.236.166.212", 9_000, Some(WEB_CATCH_ALL)),
        with_rule("87.126.240.44", 3_000, Some(YOUTUBE)),
        with_rule("1.2.3.4", 500, None),
    ])
    .unwrap();

    let f = Filter::range(T0, T0 + HOUR);
    let (rows, source) = breakdown(
        &s.conn().unwrap(),
        s.raw_dir(),
        &f,
        Dim::Rule,
        Windows::default(),
        T0 + HOUR,
    )
    .unwrap();
    assert_eq!(source, "rollup_1m");
    let by_key: HashMap<&str, _> = rows.iter().map(|r| (r.key.as_str(), r)).collect();

    let web = by_key[WEB_CATCH_ALL.to_string().as_str()];
    assert_eq!(
        (web.label.as_str(), web.bytes, web.confidence.as_deref()),
        ("HTTPS / HTTP / QUIC", 9_000, Some("guessed"))
    );
    let yt = by_key[YOUTUBE.to_string().as_str()];
    assert_eq!(
        (yt.label.as_str(), yt.bytes, yt.confidence.as_deref()),
        ("YouTube video", 3_000, Some("detected"))
    );
    let none = by_key["0"];
    assert_eq!(
        (none.label.as_str(), none.bytes, none.confidence.as_deref()),
        ("Unmatched", 500, Some("none"))
    );
}

#[test]
fn other_breakdowns_carry_no_confidence() {
    let (_d, s) = open();
    s.append_batch(&[rec(
        T0 + 1_000,
        "192.168.88.10",
        "1.2.3.4",
        443,
        10,
        1,
        Direction::Down,
    )])
    .unwrap();
    let (rows, _) = breakdown(
        &s.conn().unwrap(),
        s.raw_dir(),
        &Filter::range(T0, T0 + HOUR),
        Dim::Category,
        Windows::default(),
        T0 + HOUR,
    )
    .unwrap();
    assert!(rows.iter().all(|r| r.confidence.is_none()));
}
