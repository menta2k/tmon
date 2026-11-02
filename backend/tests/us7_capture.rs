//! Feature 002: connection naming from router packet capture (quickstart V2–V7, V10).

mod common;

use common::*;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tmon::config::Config;
use tmon_capture::CaptureHandle;
use tmon_capture::fixtures::scenarios::{EXPECTED, Expect, LATE};
use tmon_collector::listener::ListenerKind;

const RANGE: &str = "from=now-15m&to=now%2B1m";

fn enable_capture(c: &mut Config) {
    c.capture.enabled = true;
    c.capture.listen = "127.0.0.1:0".into();
    c.capture.reassembly_timeout_secs = 1;
}

async fn start_capture() -> (It, Arc<CaptureHandle>) {
    let it = start_with(enable_capture).await;
    let h = it.app.state.capture.clone().expect("capture enabled");
    assert!(h.state().listening, "{:?}", h.state());
    (it, h)
}

fn capture_addr(h: &CaptureHandle) -> String {
    h.state().bound.expect("bound").to_string()
}

async fn replay_capture(h: &CaptureHandle, names: &[&str]) {
    let to = capture_addr(h);
    for n in names {
        tmon::cli::replay::run(&fixture(&format!("capture/{n}.pcap")), &to, "max")
            .await
            .unwrap();
    }
}

/// Wait until `pred` holds for the capture counters.
async fn wait_counters(
    h: &CaptureHandle,
    pred: impl Fn(&tmon_capture::health::CounterValues) -> bool,
) {
    for _ in 0..100 {
        if pred(&h.health.counters.values()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "capture counters not reached: {:?}",
        h.health.counters.values()
    );
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

async fn flows(it: &It) -> Vec<Value> {
    it.app.flush().await.unwrap();
    let (_, b) = it.get(&format!("/flows?{RANGE}&limit=500")).await;
    b["data"].as_array().cloned().unwrap_or_default()
}

fn row<'a>(rows: &'a [Value], e: &Expect) -> Option<&'a Value> {
    let c: SocketAddr = e.client.parse().unwrap();
    rows.iter().find(|r| {
        r["local_ip"] == c.ip().to_string()
            && r["local_port"] == c.port()
            && r["remote_ip"] == e.server.split(':').next().unwrap()
    })
}

async fn wait_rows(it: &It, n: usize) -> Vec<Value> {
    for _ in 0..50 {
        let rows = flows(it).await;
        if rows.len() >= n {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("expected {n} flows");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn captured_handshakes_and_dns_name_flows() {
    let (it, h) = start_capture().await;
    replay_capture(
        &h,
        &[
            "tls_split_pq",
            "tls_ech",
            "quic_v1_rfc",
            "quic_v1_reordered",
            "quic_v2_rfc",
            "http_host",
            "shared_cdn",
            "dns_cname",
        ],
    )
    .await;
    wait_counters(&h, |c| {
        c.tls_named >= 4 && c.quic_named >= 3 && c.http_named >= 1 && c.dns_bindings >= 1
    })
    .await;
    it.replay("capture/capture_flows.pcap", ListenerKind::Netflow)
        .await;
    let rows = wait_rows(&it, EXPECTED.len()).await;
    let cats = categories(&it).await;
    for e in EXPECTED {
        let r = row(&rows, e).unwrap_or_else(|| panic!("no flow for {e:?}"));
        assert_eq!(r["remote_host"], e.host, "{e:?}");
        assert_eq!(r["remote_host_source"], e.source, "{e:?}");
        let cat = &cats[&r["category_id"].as_u64().unwrap()];
        assert_eq!(cat, e.category, "{e:?}");
    }
    let c = h.health.counters.values();
    assert_eq!(
        (c.tls_ech, c.quic_decrypt_error, c.tls_parse_error),
        (1, 0, 0)
    );

    // Hostname rules matched → "detected" confidence in the Applications breakdown.
    let (_, by_rule) = it.get(&format!("/traffic/breakdown?by=rule&{RANGE}")).await;
    let detected = by_rule["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["confidence"] == "detected")
        .count();
    assert!(detected >= 4, "{by_rule}");
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_dns_answer_backfills_an_earlier_flow() {
    let (it, h) = start_capture().await;
    it.replay("capture/capture_late_flow.pcap", ListenerKind::Netflow)
        .await;
    let rows = wait_rows(&it, 1).await;
    assert!(row(&rows, &LATE).unwrap()["remote_host"].is_null());

    replay_capture(&h, &["dns_late"]).await;
    wait_counters(&h, |c| c.dns_bindings >= 1).await;
    // Name feed runs every 5 s, then back-fills recent flows.
    for _ in 0..80 {
        let rows = flows(&it).await;
        let r = row(&rows, &LATE).unwrap();
        if r["remote_host"] == LATE.host {
            assert_eq!(r["remote_host_source"], "dns");
            it.app.shutdown().await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("late flow was not back-filled");
}

async fn wait_warning(it: &It, code: &str) -> Value {
    for _ in 0..60 {
        let (_, s) = it.get("/status").await;
        let found = s["data"]["warnings"]
            .as_array()
            .and_then(|w| w.iter().find(|w| w["code"] == code).cloned());
        if let Some(w) = found {
            return w;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let (_, s) = it.get("/status").await;
    panic!("warning {code} not raised: {}", s["data"]);
}

/// quickstart V6: a path dropping datagrams over 1,472 bytes (WSL2).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn truncated_capture_raises_the_large_packet_warning() {
    use tmon_capture::fixtures::scenarios::WSL_CONNECTIONS;
    let (it, h) = start_capture().await;
    replay_capture(&h, &["wsl_truncated"]).await;
    // Incomplete hellos are counted when the (1 s) reassembly timeout expires.
    wait_counters(&h, |c| c.tls_incomplete >= u64::from(WSL_CONNECTIONS)).await;
    let w = wait_warning(&it, "capture_large_packets_lost").await;
    assert!(w["message"].as_str().unwrap().contains("WSL2"), "{w}");
    let (_, s) = it.get("/status").await;
    let max = s["data"]["capture"]["datagrams"]["max_len_15m"]
        .as_u64()
        .unwrap();
    assert!(
        max <= 1472,
        "fixture datagrams must stay under the WSL2 limit, got {max}"
    );
    it.app.shutdown().await;
}

/// quickstart V7: FastTrack lets SYNs through but not the payload.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn syn_only_capture_raises_the_fastpath_warning() {
    use tmon_capture::fixtures::scenarios::FASTTRACK_SYNS;
    let (it, h) = start_capture().await;
    replay_capture(&h, &["fasttrack_syn_only"]).await;
    wait_counters(&h, |c| c.tcp_syn_seen >= u64::from(FASTTRACK_SYNS)).await;
    let w = wait_warning(&it, "capture_fastpath_suspected").await;
    assert!(w["message"].as_str().unwrap().contains("FastTrack"), "{w}");
    let (_, s) = it.get("/status").await;
    let codes: Vec<&str> = s["data"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["code"].as_str())
        .collect();
    assert!(!codes.contains(&"capture_large_packets_lost"), "{codes:?}");
    it.app.shutdown().await;
}

/// Random datagrams: pure noise, TZSP-framed noise, and SYNs with random tuples.
fn noise(seed: u64, n: usize) -> impl Iterator<Item = Vec<u8>> {
    use tmon_capture::fixtures::{SYN, tcp, tzsp_ip};
    let mut x = seed | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    (0..n).map(move |i| {
        let r = next();
        let len = (r % 1400) as usize;
        let body: Vec<u8> = (0..len).map(|j| (next() >> (j % 8)) as u8).collect();
        match i % 3 {
            0 => body,
            1 => tzsp_ip(&body),
            _ => {
                let c = SocketAddr::new(
                    [192, 168, 88, (r % 250) as u8 + 2].into(),
                    (r >> 16) as u16 | 1024,
                );
                let s = SocketAddr::new([203, 0, 113, (r >> 8) as u8].into(), 443);
                tcp(c, s, r as u32, SYN, &[])
            }
        }
    })
}

/// FR-015 / SC-006: a capture flood never costs flow records.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capture_flood_does_not_drop_flows() {
    const FLOOD: usize = 200_000;
    let (it, h) = start_capture().await;
    let to = h.state().bound.unwrap();
    let blast = tokio::task::spawn_blocking(move || {
        let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        noise(0x5eed, FLOOD)
            .filter(|d| s.send_to(d, to).is_ok())
            .count()
    });
    let total: u64 = truth_by("mixed-home.truth.csv", 0).values().sum();
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    let sent = blast.await.unwrap();
    assert_eq!(sent, FLOOD);
    it.wait_for_bytes(&format!("/traffic/breakdown?by=category&{RANGE}"), total)
        .await;

    let (_, s) = it.get("/status").await;
    let d = &s["data"];
    let flow_drops: u64 = d["ingest"]["dropped_total"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_u64)
        .sum();
    assert_eq!(flow_drops, 0, "{}", d["ingest"]);
    let c = h.health.counters.values();
    let seen = c.datagrams_received + c.dropped_backpressure + c.rejected_source;
    assert!(seen > 0, "{c:?}");
    assert!(
        c.malformed_tzsp + c.malformed_frame + c.unsupported() > 0,
        "{c:?}"
    );
    assert!(c.tcp_syn_seen > 0, "{c:?}");
    let limits = h.settings.limits;
    assert!(h.table.conns_len() <= limits.max_connections);
    let cap = &d["capture"];
    assert!(
        cap["connections"]["reassembly_bytes"].as_u64().unwrap()
            <= limits.max_reassembly_bytes_total as u64
    );
    it.app.shutdown().await;
}

/// quickstart V1: without `[capture]`, nothing changes from feature 001.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_capture_changes_nothing() {
    let it = start().await;
    assert!(it.app.state.capture.is_none());
    // No capture socket: the default port is free (unless another process holds it).
    if let Err(e) = std::net::UdpSocket::bind("0.0.0.0:37008") {
        eprintln!("37008 busy outside this test ({e}); skipping the port check");
    }
    let (_, s) = it.get("/status").await;
    assert_eq!(
        s["data"]["capture"],
        serde_json::json!({ "enabled": false })
    );
    let codes: Vec<&str> = s["data"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["code"].as_str())
        .collect();
    assert!(
        !codes.iter().any(|c| c.starts_with("capture_")),
        "{codes:?}"
    );

    let total: u64 = truth_by("mixed-home.truth.csv", 0).values().sum();
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    it.wait_for_bytes(&format!("/traffic/breakdown?by=category&{RANGE}"), total)
        .await;
    let rows = flows(&it).await;
    assert!(!rows.is_empty());
    for r in &rows {
        let src = &r["remote_host_source"];
        assert!(
            src.is_null() || src == "router",
            "unexpected name source {src} in {r}"
        );
    }
    it.app.shutdown().await;
}
