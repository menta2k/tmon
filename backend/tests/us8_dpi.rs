//! Feature 003: application classification end to end (quickstart V1, US1).
//! Uses the pure-Rust `fake_dpi` example (built by `cargo test --workspace`);
//! the `#[ignore]` variant runs the real `tmon-dpi` from `TMON_DPI_BIN`.

mod common;

use common::*;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tmon::config::Config;
use tmon_capture::fixtures::apps::{APPS, AppExpect, PORT_ONLY, ZOOM};
use tmon_collector::listener::ListenerKind;
use tmon_core::ConnectionClassifier;
use tmon_core::naming::ConnKey;
use tmon_dpi_client::{DpiHandle, DpiState};

const RANGE: &str = "from=now-15m&to=now%2B1m";

/// Tests here share one process; serialize them so the child-process check
/// of the disabled-mode test cannot see another test's classifier.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What the classifier under test should report for a fixture scenario.
struct Want {
    scenario: &'static str,
    app: &'static str,
    confident: bool,
    category: &'static str,
    host: Option<(&'static str, &'static str)>,
}

const fn want(
    scenario: &'static str,
    app: &'static str,
    confident: bool,
    category: &'static str,
) -> Want {
    Want {
        scenario,
        app,
        confident,
        category,
        host: None,
    }
}

/// The fake classifier's verdicts (`fake_dpi.rs`) for the app fixtures.
const FAKE: [Want; 7] = [
    want("bittorrent", "BitTorrent", true, "File Sharing"),
    want("wireguard", "WireGuard", false, "VPN/Tunnels"),
    want("openvpn", "OpenVPN", true, "VPN/Tunnels"),
    want("stun_rtp", "STUN", true, "Messaging/VoIP"),
    want("steam_a2s", "Steam", true, "Gaming"),
    Want {
        host: Some(("desktop.example.net", "dpi")),
        ..want("rdp", "RDP", true, "Remote Access")
    },
    // Feature 002's TLS name stays; the classifier's host is ignored.
    Want {
        host: Some(("zoom.us", "tls")),
        ..want("zoom_tls", "Zoom", true, "Messaging/VoIP")
    },
];

/// `TMON_FAKE_DPI` (set by `just coverage`, whose target dir has no plain
/// example build) or `examples/fake_dpi` next to this test binary.
fn fake_dpi() -> PathBuf {
    if let Some(p) = std::env::var_os("TMON_FAKE_DPI") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().unwrap();
    let p = exe
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("fake_dpi");
    assert!(
        p.exists(),
        "{} missing: run `cargo build -p tmon-dpi-client --example fake_dpi` (or `cargo test --workspace`)",
        p.display()
    );
    p
}

fn enable(binary: String) -> impl FnOnce(&mut Config) {
    move |c| {
        c.capture.enabled = true;
        c.capture.listen = "127.0.0.1:0".into();
        c.capture.reassembly_timeout_secs = 1;
        c.dpi.enabled = true;
        c.dpi.binary = binary;
    }
}

fn scenario(name: &str) -> AppExpect {
    APPS.iter()
        .copied()
        .chain([ZOOM, PORT_ONLY])
        .find(|e| e.scenario == name)
        .unwrap_or_else(|| panic!("scenario {name}"))
}

fn key(e: &AppExpect) -> ConnKey {
    let (c, s): (SocketAddr, SocketAddr) = (e.client.parse().unwrap(), e.server.parse().unwrap());
    ConnKey {
        lan_ip: c.ip(),
        lan_port: c.port(),
        remote_ip: s.ip(),
        remote_port: s.port(),
        proto: e.proto,
    }
}

async fn wait(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    for _ in 0..secs * 20 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn start_dpi(binary: String) -> (It, Arc<DpiHandle>) {
    let it = start_with(enable(binary)).await;
    let h = it.app.state.dpi.clone().expect("dpi enabled");
    wait("classifier running", 10, || {
        h.health.state() == DpiState::Running
    })
    .await;
    (it, h)
}

async fn replay_capture(it: &It, name: &str) {
    let to = it
        .app
        .state
        .capture
        .as_ref()
        .unwrap()
        .state()
        .bound
        .unwrap()
        .to_string();
    tmon::cli::replay::run(&fixture(&format!("capture/{name}.pcap")), &to, "max")
        .await
        .unwrap();
}

async fn categories(it: &It) -> HashMap<u64, String> {
    let (_, cats) = it.get("/categories").await;
    cats["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["id"].as_u64().unwrap(),
                c["name"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

async fn rows(it: &It) -> Vec<Value> {
    it.app.flush().await.unwrap();
    let (_, b) = it.get(&format!("/flows?{RANGE}&limit=500")).await;
    b["data"].as_array().cloned().unwrap_or_default()
}

fn row<'a>(rows: &'a [Value], e: &AppExpect) -> Option<&'a Value> {
    let k = key(e);
    rows.iter().find(|r| {
        r["local_ip"] == k.lan_ip.to_string()
            && r["local_port"] == k.lan_port
            && r["remote_ip"] == k.remote_ip.to_string()
    })
}

async fn wait_rows(it: &It, n: usize) -> Vec<Value> {
    for _ in 0..50 {
        let r = rows(it).await;
        if r.len() >= n {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("expected {n} flows");
}

fn check(r: &Value, w: &Want, cats: &HashMap<u64, String>) {
    let s = w.scenario;
    assert_eq!(r["app"], w.app, "{s}: {r}");
    assert_eq!(r["app_confident"], w.confident, "{s}");
    assert_eq!(cats[&r["category_id"].as_u64().unwrap()], w.category, "{s}");
    match w.host {
        Some((host, source)) => {
            assert_eq!(r["remote_host"], host, "{s}");
            assert_eq!(r["remote_host_source"], source, "{s}");
        }
        None => assert!(r["remote_host_source"] != "dpi", "{s}: {r}"),
    }
}

/// Classify the mix, store its flows, then classify one connection late.
async fn run(binary: String, wants: &[Want], late: Option<Want>) {
    let _serial = SERIAL.lock().await;
    let (it, h) = start_dpi(binary).await;
    replay_capture(&it, "apps_mix").await;
    replay_capture(&it, "apps_zoom_tls").await;
    for w in wants {
        let k = key(&scenario(w.scenario));
        wait(w.scenario, 10, || {
            h.table.classify(&k, tmon_dpi_client::now_ms()).is_some()
        })
        .await;
    }
    it.replay("capture/apps_flows.pcap", ListenerKind::Netflow)
        .await;
    let stored = wait_rows(&it, APPS.len() + 2).await;
    let cats = categories(&it).await;
    for w in wants {
        let r = row(&stored, &scenario(w.scenario))
            .unwrap_or_else(|| panic!("no flow for {}", w.scenario));
        check(r, w, &cats);
    }
    if let Some(w) = late {
        let e = scenario(w.scenario);
        assert!(
            row(&stored, &e).unwrap()["app"].is_null(),
            "not yet classified"
        );
        replay_capture(&it, "apps_port_only").await;
        // Back-fill drains every 5 s (FR-008: within 10 s).
        let mut found = None;
        for _ in 0..60 {
            let r = rows(&it).await;
            if let Some(x) = row(&r, &e).filter(|x| !x["app"].is_null()) {
                found = Some(x.clone());
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        check(&found.expect("late classification back-filled"), &w, &cats);
    }
    let (_, b) = it.get(&format!("/traffic/breakdown?by=app&{RANGE}")).await;
    assert!(
        b["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["key"] == wants[0].app),
        "{b}"
    );
    let (_, st) = it.get("/status").await;
    assert_eq!(st["data"]["dpi"]["state"], "running", "{st}");
    assert!(st["data"]["dpi"]["queue"]["sent"].as_u64().unwrap() > 0);
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fake_classifier_names_apps_end_to_end() {
    let late = want("port_only", "WireGuard", false, "VPN/Tunnels");
    run(fake_dpi().display().to_string(), &FAKE, Some(late)).await;
}

/// SC-002 with real nDPI: `TMON_DPI_BIN=backend/dpi/target/release/tmon-dpi`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs the real tmon-dpi (just dpi-build) and TMON_DPI_BIN"]
async fn real_ndpi_names_apps_end_to_end() {
    let bin = std::env::var("TMON_DPI_BIN").expect("TMON_DPI_BIN");
    let wants: Vec<Want> = APPS
        .iter()
        .map(|e| want(e.scenario, e.app, true, e.category))
        .collect();
    run(bin, &wants, None).await;
}

/// Names of this process's child processes (Linux `/proc`).
fn children() -> Vec<String> {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return Vec::new();
    };
    tasks
        .flatten()
        .filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok())
        .flat_map(|s| s.split_whitespace().map(String::from).collect::<Vec<_>>())
        .filter_map(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
        .map(|c| c.trim().to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_dpi_spawns_nothing_and_changes_nothing() {
    let _serial = SERIAL.lock().await;
    let it = start_with(|c| {
        c.capture.enabled = true;
        c.capture.listen = "127.0.0.1:0".into();
        c.capture.reassembly_timeout_secs = 1;
    })
    .await;
    assert!(it.app.state.dpi.is_none());
    // A classifier of an earlier test may still be exiting.
    wait("no classifier child", 5, || {
        !children().iter().any(|c| c.contains("dpi"))
    })
    .await;
    replay_capture(&it, "apps_mix").await;
    replay_capture(&it, "apps_zoom_tls").await;
    it.replay("capture/apps_flows.pcap", ListenerKind::Netflow)
        .await;
    let stored = wait_rows(&it, APPS.len() + 2).await;
    assert!(
        stored
            .iter()
            .all(|r| r["app"].is_null() && r["app_confident"].is_null())
    );
    let zoom = row(&stored, &ZOOM).unwrap();
    assert_eq!(
        (
            zoom["remote_host"].as_str(),
            zoom["remote_host_source"].as_str()
        ),
        (Some("zoom.us"), Some("tls"))
    );
    let (_, st) = it.get("/status").await;
    assert_eq!(st["data"]["dpi"], serde_json::json!({"enabled": false}));
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_binary_warns_and_tmon_keeps_serving() {
    let _serial = SERIAL.lock().await;
    let it = start_with(enable("/nonexistent/tmon-dpi".into())).await;
    let h = it.app.state.dpi.clone().unwrap();
    wait("not_found", 10, || h.health.state() == DpiState::NotFound).await;
    let (s, st) = it.get("/status").await;
    assert_eq!(s, 200);
    let codes: Vec<&str> = st["data"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["code"].as_str())
        .collect();
    assert!(codes.contains(&"dpi_not_found"), "{codes:?}");
    it.replay("capture/apps_flows.pcap", ListenerKind::Netflow)
        .await;
    let stored = wait_rows(&it, APPS.len() + 2).await;
    assert!(stored.iter().all(|r| r["app"].is_null()));
    it.app.shutdown().await;
}
