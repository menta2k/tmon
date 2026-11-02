//! Packets reach the classifier; answers reach the table, back-fill buffer
//! and final set (T031).

mod common;

use common::*;
use std::time::Duration;
use tmon_capture::{PacketTap, TapDir};
use tmon_core::app::ConnectionClassifier;
use tmon_dpi_client::DpiState;

#[tokio::test]
async fn classifications_flow_into_table_pending_and_final_set() {
    let r = start(settings(&[]), fast());
    assert!(
        wait_for(Duration::from_secs(5), || r.handle.health.state()
            == DpiState::Running)
        .await
    );
    let tap = &r.handle.tap;
    let bt = key(6881);
    let wg = key(51820);
    tap.offer(&bt, TapDir::LanToRemote, now(), &l3(&bt));
    tap.offer(&wg, TapDir::LanToRemote, now(), &l3(&wg));
    assert!(wait_for(Duration::from_secs(3), || r.handle.table.len() == 2).await);
    let a = r.handle.table.classify(&bt, now()).unwrap();
    assert_eq!(
        (a.app.as_ref(), a.confident, a.final_),
        ("BitTorrent", true, true)
    );
    let w = r.handle.table.classify(&wg, now()).unwrap();
    assert_eq!((w.app.as_ref(), w.confident), ("WireGuard", false));
    assert_eq!(r.handle.pending.drain().len(), 2);

    // BitTorrent is final: further packets are skipped at the tap.
    let before = tap.counters.values();
    tap.offer(&bt, TapDir::RemoteToLan, now(), &l3(&bt));
    let after = tap.counters.values();
    assert_eq!(after.skipped_final, before.skipped_final + 1);
    assert_eq!(after.sent, before.sent);
    r.shutdown().await;
}

#[tokio::test]
async fn offers_while_down_count_dropped_down() {
    let s = tmon_dpi_client::DpiSettings {
        binary: "/nonexistent".into(),
        ..tmon_dpi_client::DpiSettings::default()
    };
    let r = start(s, fast());
    let k = key(1);
    r.handle.tap.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
    let v = r.handle.tap.counters.values();
    assert_eq!((v.dropped_down, v.sent), (1, 0));
    r.shutdown().await;
}

#[tokio::test]
async fn per_connection_packet_budget() {
    let mut s = settings(&[]);
    s.max_packets_per_conn = 4;
    let r = start(s, fast());
    assert!(
        wait_for(Duration::from_secs(5), || r.handle.health.state()
            == DpiState::Running)
        .await
    );
    let k = key(443);
    for _ in 0..10 {
        r.handle.tap.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
    }
    let v = r.handle.tap.counters.values();
    assert_eq!((v.sent, v.skipped_final), (4, 6));
    r.shutdown().await;
}

#[tokio::test]
async fn heartbeat_stats_are_reported() {
    let r = start(settings(&["--stats-ms", "50"]), fast());
    assert!(
        wait_for(Duration::from_secs(5), || r.handle.health.state()
            == DpiState::Running)
        .await
    );
    let k = key(27015);
    r.handle.tap.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
    assert!(
        wait_for(Duration::from_secs(3), || r
            .handle
            .status(now())
            .classifier
            .packets_in
            == 1)
        .await
    );
    let st = r.handle.status(now());
    assert_eq!(st.classifier.mem_limit_bytes, 192 << 20);
    assert_eq!(st.top_apps[0].app, "BitTorrent");
    assert!(st.uptime_secs.is_some());
    r.shutdown().await;
}
