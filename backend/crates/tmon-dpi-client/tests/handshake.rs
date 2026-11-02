//! Spawn and handshake (FR-016, FR-017) with the fake classifier.

mod common;

use common::*;
use std::time::Duration;
use tmon_dpi_client::{DpiSettings, DpiState};

#[tokio::test]
async fn valid_hello_reaches_running_with_catalog() {
    let r = start(settings(&[]), fast());
    assert!(
        wait_for(Duration::from_secs(5), || r.handle.health.state()
            == DpiState::Running)
        .await
    );
    let st = r.handle.status(now());
    assert_eq!(st.engine.as_deref(), Some("fake 1.0"));
    assert_eq!(st.proto_version, Some(1));
    assert!(r.handle.tap.is_running());
    let cat = r.handle.catalog();
    assert!(cat.iter().any(|c| c.app == "BitTorrent"));
    assert!(
        cat.windows(2)
            .all(|w| w[0].app.to_lowercase() <= w[1].app.to_lowercase())
    );
    r.shutdown().await;
}

#[tokio::test]
async fn wrong_version_is_incompatible_without_restart() {
    let r = start(settings(&["--proto-version", "2"]), fast());
    assert!(
        wait_for(Duration::from_secs(5), || r.handle.health.state()
            == DpiState::Incompatible)
        .await
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let st = r.handle.status(now());
    assert_eq!(st.state, DpiState::Incompatible);
    assert_eq!(st.restarts_total, 0);
    let w = st.warnings();
    assert_eq!(w[0].code(), "dpi_incompatible");
    assert!(w[0].message().contains("v2") && w[0].message().contains("v1"));
    assert!(
        r.task.is_finished() || {
            r.shutdown().await;
            true
        }
    );
}

#[tokio::test]
async fn missing_binary_is_not_found() {
    let s = DpiSettings {
        binary: "/nonexistent/tmon-dpi".into(),
        ..DpiSettings::default()
    };
    let r = start(s, fast());
    assert!(
        wait_for(Duration::from_secs(2), || r.handle.health.state()
            == DpiState::NotFound)
        .await
    );
    let w = r.handle.warnings(now());
    assert_eq!(w[0].code(), "dpi_not_found");
    assert!(w[0].message().contains("/nonexistent/tmon-dpi"));
    let s = DpiSettings {
        binary: "no-such-tmon-dpi-binary".into(),
        ..DpiSettings::default()
    };
    let r2 = start(s, fast());
    assert!(
        wait_for(Duration::from_secs(2), || r2.handle.health.state()
            == DpiState::NotFound)
        .await
    );
    r.shutdown().await;
    r2.shutdown().await;
}

#[tokio::test]
async fn silent_classifier_fails_the_start() {
    let r = start(settings(&["--mode", "silent"]), fast());
    assert!(
        wait_for(Duration::from_secs(5), || r
            .handle
            .status(now())
            .restarts_total
            >= 1)
        .await
    );
    assert_ne!(r.handle.health.state(), DpiState::Running);
    assert!(r.handle.status(now()).error.unwrap().contains("no Hello"));
    r.shutdown().await;
}

#[test]
fn resolve_finds_paths_and_path_entries() {
    use tmon_dpi_client::spawn::resolve;
    assert!(resolve(&fake_dpi().display().to_string()).is_some());
    assert!(resolve("/etc/hostname").is_none(), "not executable");
    assert!(resolve("sh").is_some(), "found in PATH");
}

#[test]
fn args_follow_the_contract() {
    let s = DpiSettings {
        extra_args: vec!["--mode".into(), "garbage".into()],
        ..DpiSettings::default()
    };
    assert_eq!(
        s.args().join(" "),
        "--fd 3 --max-flows 50000 --max-packets-per-conn 32 --idle-timeout-ms 900000 \
         --memory-limit-mb 192 --mode garbage"
    );
    assert_eq!(s.app_table_capacity(), 100_000);
}
