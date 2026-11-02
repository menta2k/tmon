//! Status snapshot and warnings (T062).

use tmon_dpi_client::health::{DROP_WINDOW_MS, DpiHealth};
use tmon_dpi_client::tap::TapValues;
use tmon_dpi_client::{DpiState, DpiWarning};
use tmon_dpi_proto::{Hello, PROTO_VERSION, Stats, TopApp};

const T0: i64 = 1_767_225_600_000;

fn tap(sent: u64, dropped: u64) -> TapValues {
    TapValues {
        sent,
        dropped_queue: dropped,
        ..TapValues::default()
    }
}

fn hello() -> Hello {
    Hello {
        proto_version: PROTO_VERSION,
        impl_name: "tmon-dpi".into(),
        impl_version: "0.1.0".into(),
        engine: "nDPI 6.0".into(),
        max_flows: 10,
        catalog: vec![],
    }
}

#[test]
fn snapshot_reports_engine_uptime_and_classifier_stats() {
    let h = DpiHealth::new("tmon-dpi");
    assert_eq!(h.state(), DpiState::Starting);
    h.running(&hello(), T0);
    h.stats(
        Stats {
            packets_in: 7,
            evicted_lru: 1,
            evicted_idle: 2,
            evicted_memory: 3,
            top: vec![TopApp {
                app: "Steam".into(),
                flows: 4,
            }],
            ..Stats::default()
        },
        T0,
    );
    let s = h.snapshot(tap(10, 0), T0 + 61_000);
    assert_eq!(s.state, DpiState::Running);
    assert_eq!(s.engine.as_deref(), Some("nDPI 6.0"));
    assert_eq!(s.uptime_secs, Some(61));
    assert_eq!(s.classifier.packets_in, 7);
    assert_eq!(s.classifier.evicted, 6);
    assert_eq!(s.top_apps[0].flows, 4);
    assert!(s.warnings().is_empty());
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["state"], "running");
    assert!(v.get("binary").is_none());
}

#[test]
fn drop_share_over_the_window_warns() {
    let h = DpiHealth::new("x");
    h.sample(tap(0, 0), T0);
    let s = h.snapshot(tap(900, 100), T0 + 60_000);
    assert!((s.queue.drop_share_15m - 0.1).abs() < 1e-9);
    let w = s.warnings();
    assert!(matches!(w[0], DpiWarning::Dropping { .. }));
    assert!(w[0].message().contains("10.0%"));
    // Old samples leave the window.
    for m in 1..=20 {
        h.sample(tap(900 + m * 1_000, 100), T0 + m as i64 * 60_000);
    }
    let later = h.snapshot(tap(21_000, 100), T0 + 20 * 60_000 + DROP_WINDOW_MS / 15);
    assert!(later.queue.drop_share_15m < 0.05);
}

#[test]
fn failures_inside_the_window_only() {
    let h = DpiHealth::new("x");
    assert_eq!(h.failure("a".into(), T0), 1);
    assert_eq!(h.failure("b".into(), T0 + 1_000), 2);
    assert_eq!(h.failure("c".into(), T0 + 11 * 60_000), 1);
    let s = h.snapshot(tap(0, 0), T0 + 11 * 60_000);
    assert_eq!(s.restarts_total, 3);
    assert_eq!(s.restarts_10m, 1);
    assert_eq!(s.error.as_deref(), Some("c"));
}

#[test]
fn terminal_states_warn_with_hints() {
    let h = DpiHealth::new("/opt/tmon-dpi");
    h.set_state(DpiState::NotFound, None);
    let w = h.snapshot(tap(0, 0), T0).warnings();
    assert_eq!(w[0].code(), "dpi_not_found");
    assert!(w[0].message().contains("/opt/tmon-dpi"));
    h.set_state(DpiState::Failed, Some("boom".into()));
    let w = h.snapshot(tap(0, 0), T0).warnings();
    assert_eq!(w[0].code(), "dpi_failed");
    assert!(w[0].message().contains("boom") && w[0].message().contains("Restart tmon"));
    h.incompatible(9);
    let w = h.snapshot(tap(0, 0), T0).warnings();
    assert_eq!(w[0].code(), "dpi_incompatible");
    assert!(DpiState::Failed.is_terminal() && !DpiState::Running.is_terminal());
    assert_eq!(DpiState::NotFound.as_str(), "not_found");
}

#[test]
fn catalog_is_capped() {
    let h = DpiHealth::new("x");
    let entries: Vec<_> = (0..3_000)
        .map(|i| tmon_dpi_proto::CatalogEntry {
            app: format!("a{i}"),
            category: String::new(),
        })
        .collect();
    h.add_catalog(&entries);
    assert_eq!(h.catalog().len(), tmon_dpi_proto::MAX_CATALOG);
}
