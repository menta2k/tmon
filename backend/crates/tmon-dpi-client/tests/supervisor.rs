//! Restart with back-off, give-up, heartbeat and invalid-input limits (T061).

mod common;

use common::*;
use std::time::Duration;
use tmon_capture::{PacketTap, TapDir};
use tmon_dpi_client::{DpiState, Timing};

async fn running(r: &Running) {
    assert!(
        wait_for(Duration::from_secs(5), || r.handle.health.state()
            == DpiState::Running)
        .await
    );
}

fn poke(r: &Running, n: u16) {
    for p in 0..n {
        let k = key(10_000 + p);
        r.handle.tap.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
    }
}

#[test]
fn backoff_doubles_and_caps() {
    let t = Timing::default();
    let secs: Vec<u64> = (1..=8).map(|n| t.backoff(n).as_secs()).collect();
    assert_eq!(secs, [1, 2, 4, 8, 16, 32, 60, 60]);
}

#[tokio::test]
async fn crash_restarts_then_runs_again() {
    let r = start(settings(&["--mode", "crash_after=3"]), fast());
    running(&r).await;
    poke(&r, 3);
    assert!(
        wait_for(Duration::from_secs(5), || r
            .handle
            .status(now())
            .restarts_total
            == 1)
        .await
    );
    running(&r).await;
    let st = r.handle.status(now());
    assert!(st.error.unwrap().contains("exited"));
    assert_eq!(st.restarts_10m, 1);
    r.shutdown().await;
}

#[tokio::test]
async fn five_failures_in_ten_minutes_give_up() {
    let r = start(settings(&["--mode", "crash_after=1"]), fast());
    for _ in 0..5 {
        running(&r).await;
        let before = r.handle.status(now()).restarts_total;
        poke(&r, 1);
        assert!(
            wait_for(Duration::from_secs(5), || r
                .handle
                .status(now())
                .restarts_total
                > before)
            .await
        );
    }
    assert!(
        wait_for(Duration::from_secs(2), || r.handle.health.state()
            == DpiState::Failed)
        .await
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let st = r.handle.status(now());
    assert_eq!(st.restarts_total, 5);
    let w = st.warnings();
    assert_eq!(w[0].code(), "dpi_failed");
    assert!(r.task.is_finished());
}

#[tokio::test]
async fn missing_heartbeat_restarts() {
    let r = start(
        settings(&["--mode", "hang_after=1", "--stats-ms", "100"]),
        fast(),
    );
    running(&r).await;
    poke(&r, 1);
    assert!(
        wait_for(Duration::from_secs(5), || r
            .handle
            .status(now())
            .restarts_total
            >= 1)
        .await
    );
    assert!(r.handle.status(now()).error.unwrap().contains("heartbeat"));
    r.shutdown().await;
}

#[tokio::test]
async fn invalid_messages_are_counted_and_limit_restarts() {
    let r = start(
        settings(&["--mode", "garbage", "--stats-ms", "100"]),
        fast(),
    );
    assert!(
        wait_for(Duration::from_secs(5), || r
            .handle
            .status(now())
            .ipc_invalid
            >= 101)
        .await
    );
    assert!(
        wait_for(Duration::from_secs(5), || r
            .handle
            .status(now())
            .restarts_total
            >= 1)
        .await
    );
    assert!(r.handle.status(now()).error.unwrap().contains("invalid"));
    r.shutdown().await;
}

#[tokio::test]
async fn restarting_warning_after_two_restarts() {
    let r = start(settings(&["--mode", "crash_after=1"]), fast());
    for _ in 0..2 {
        running(&r).await;
        let before = r.handle.status(now()).restarts_total;
        poke(&r, 1);
        assert!(
            wait_for(Duration::from_secs(5), || r
                .handle
                .status(now())
                .restarts_total
                > before)
            .await
        );
    }
    let codes: Vec<&str> = r.handle.warnings(now()).iter().map(|w| w.code()).collect();
    assert!(codes.contains(&"dpi_restarting"), "{codes:?}");
    r.shutdown().await;
}

#[tokio::test]
async fn stop_terminates_the_classifier() {
    let r = start(settings(&[]), fast());
    running(&r).await;
    let task = r.task;
    r.stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert!(!r.handle.tap.is_running());
}
