//! Application filter, `by=app` breakdown, flow rows and host top apps (feature 003, US2).

mod common;

use common::*;
use std::collections::HashMap;
use tmon_core::app::AppInfo;
use tmon_core::{Direction, FlowRecord};
use tmon_store::duck::query::flows::{CSV_HEADER, PageReq, search};
use tmon_store::duck::query::host::top_apps;
use tmon_store::duck::query::router::{Dim, Filter, Windows};
use tmon_store::duck::query::traffic::breakdown;
use tmon_store::{FlowStore, StoreError};

const NOW: i64 = T0 + HOUR;

fn app(f: FlowRecord, name: &str, confident: bool) -> FlowRecord {
    f.with_app(&AppInfo::new(name, "Download", confident, None, false).unwrap())
}

fn seed() -> (tempfile::TempDir, std::sync::Arc<tmon_store::DuckStore>) {
    let (d, s) = open();
    let r = |local: &str, bytes: u64| {
        rec(
            T0 + 1_000,
            local,
            "203.0.113.5",
            6881,
            bytes,
            1,
            Direction::Down,
        )
    };
    s.append_batch(&[
        app(r("192.168.88.10", 5_000), "BitTorrent", true),
        app(r("192.168.88.10", 3_000), "BitTorrent", false),
        app(r("192.168.88.11", 2_000), "WireGuard", false),
        r("192.168.88.10", 1_000),
    ])
    .unwrap();
    (d, s)
}

fn apps_filter(apps: &[&str]) -> Filter {
    Filter {
        apps: apps.iter().map(|a| a.to_string()).collect(),
        ..Filter::range(T0, NOW)
    }
}

#[test]
fn flow_rows_carry_the_app_and_csv_appends_columns() {
    let (_d, s) = seed();
    let (rows, total) = search(
        &s.conn().unwrap(),
        s.raw_dir(),
        &Filter::range(T0, NOW),
        PageReq::first(100),
        Windows::default(),
        NOW,
    )
    .unwrap();
    assert_eq!(total, 4);
    let bt = rows.iter().find(|r| r.bytes == 5_000).unwrap();
    assert_eq!(
        (bt.app.as_deref(), bt.app_confident),
        (Some("BitTorrent"), Some(true))
    );
    let none = rows.iter().find(|r| r.bytes == 1_000).unwrap();
    assert_eq!((none.app.as_deref(), none.app_confident), (None, None));
    assert_eq!(
        &CSV_HEADER[CSV_HEADER.len() - 3..],
        ["remote_host_source", "app", "app_confident"]
    );
    let fields = bt.csv_fields();
    assert_eq!(fields.len(), CSV_HEADER.len());
    assert_eq!(&fields[fields.len() - 2..], ["BitTorrent", "true"]);
    assert_eq!(&none.csv_fields()[CSV_HEADER.len() - 2..], ["", ""]);
}

#[test]
fn app_filter_is_case_insensitive_and_raw_only() {
    let (_d, s) = seed();
    let conn = s.conn().unwrap();
    let (rows, total) = search(
        &conn,
        s.raw_dir(),
        &apps_filter(&["bittorrent"]),
        PageReq::first(100),
        Windows::default(),
        NOW,
    )
    .unwrap();
    assert_eq!(total, 2);
    assert!(rows.iter().all(|r| r.app.as_deref() == Some("BitTorrent")));
    let w = Windows {
        raw_ms: HOUR / 2,
        ..Windows::default()
    };
    let old = breakdown(
        &conn,
        s.raw_dir(),
        &apps_filter(&["BitTorrent"]),
        Dim::Category,
        w,
        NOW,
    );
    assert!(matches!(
        old,
        Err(StoreError::UnsupportedFilterRange { .. })
    ));
}

#[test]
fn breakdown_by_app_reports_confident_share() {
    let (_d, s) = seed();
    let (rows, source) = breakdown(
        &s.conn().unwrap(),
        s.raw_dir(),
        &Filter::range(T0, NOW),
        Dim::App,
        Windows::default(),
        NOW,
    )
    .unwrap();
    assert_eq!(source, "raw");
    let by: HashMap<&str, _> = rows.iter().map(|r| (r.key.as_str(), r)).collect();
    let bt = by["BitTorrent"];
    assert_eq!(
        (bt.bytes, bt.flows, bt.confident_share),
        (8_000, 2, Some(0.5))
    );
    assert_eq!(by["WireGuard"].confident_share, Some(0.0));
    let unknown = by[""];
    assert_eq!(
        (unknown.label.as_str(), unknown.bytes),
        ("Unclassified", 1_000)
    );
    assert!(rows.iter().all(|r| r.confidence.is_none()));
}

#[test]
fn breakdown_by_app_beyond_raw_retention_is_unsupported() {
    let (_d, s) = seed();
    let w = Windows {
        raw_ms: HOUR / 2,
        ..Windows::default()
    };
    let r = breakdown(
        &s.conn().unwrap(),
        s.raw_dir(),
        &Filter::range(T0, NOW),
        Dim::App,
        w,
        NOW,
    );
    assert!(matches!(r, Err(StoreError::UnsupportedFilterRange { .. })));
}

#[test]
fn host_top_apps_lists_classified_apps_by_bytes() {
    let (_d, s) = seed();
    let conn = s.conn().unwrap();
    let f = Filter::range(T0, NOW);
    let top = top_apps(
        &conn,
        s.raw_dir(),
        "192.168.88.10",
        &f,
        Windows::default(),
        NOW,
    )
    .unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(
        (top[0].app.as_str(), top[0].bytes, top[0].flows),
        ("BitTorrent", 8_000, 2)
    );
    let w = Windows {
        raw_ms: HOUR / 2,
        ..Windows::default()
    };
    assert!(
        top_apps(&conn, s.raw_dir(), "192.168.88.10", &f, w, NOW)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn empty_app_selects_unclassified_flows_alone_or_with_names() {
    let (_d, s) = seed();
    let conn = s.conn().unwrap();
    let total = |apps: &[&str]| {
        search(
            &conn,
            s.raw_dir(),
            &apps_filter(apps),
            PageReq::first(100),
            Windows::default(),
            NOW,
        )
        .unwrap()
        .1
    };
    assert_eq!(total(&[""]), 1);
    assert_eq!(total(&["", "wireguard"]), 2);
}

#[test]
fn flows_sort_by_bytes_or_packets_either_way() {
    use tmon_store::duck::query::flows::{Sort, SortKey};
    let (_d, s) = seed();
    let conn = s.conn().unwrap();
    let bytes = |sort: Sort| {
        let req = PageReq {
            page: 1,
            limit: 100,
            sort,
        };
        search(
            &conn,
            s.raw_dir(),
            &Filter::range(T0, NOW),
            req,
            Windows::default(),
            NOW,
        )
        .unwrap()
        .0
        .iter()
        .map(|r| r.bytes)
        .collect::<Vec<_>>()
    };
    let by = |key, desc| Sort { key, desc };
    assert_eq!(
        bytes(by(SortKey::Bytes, true)),
        [5_000, 3_000, 2_000, 1_000]
    );
    assert_eq!(
        bytes(by(SortKey::Bytes, false)),
        [1_000, 2_000, 3_000, 5_000]
    );
    // All seeded flows have 1 packet: ties are broken deterministically.
    assert_eq!(bytes(by(SortKey::Packets, true)).len(), 4);
    assert_eq!(Sort::default(), by(SortKey::Time, true));
    assert_eq!(SortKey::parse("bytes"), Some(SortKey::Bytes));
    assert_eq!(SortKey::parse("bytes; DROP"), None);
}
