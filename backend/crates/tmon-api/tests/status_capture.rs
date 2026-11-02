//! `/status.capture`, capture warnings and `tmon_capture_*` metrics (T049).

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tmon_capture::health::inc;
use tmon_capture::{CaptureHandle, CaptureSettings};
use tokio::sync::watch;

struct Rig {
    t: TestApp,
    h: Arc<CaptureHandle>,
    now: Arc<AtomicI64>,
    _shutdown: watch::Sender<bool>,
}

fn settings(listen: &str) -> CaptureSettings {
    CaptureSettings {
        listen: listen.parse().unwrap(),
        ..CaptureSettings::default()
    }
}

fn rig(listen: &str) -> Rig {
    let now = Arc::new(AtomicI64::new(tmon_capture::listener::now_ms()));
    let clock = now.clone();
    let mut t = app_with_clock(Arc::new(move || clock.load(Ordering::SeqCst)));
    let (tx, rx) = watch::channel(false);
    let h = tmon_capture::listener::start(settings(listen), rx);
    t.state.capture = Some(h.clone());
    t.app = tmon_api::router(t.state.clone());
    Rig {
        t,
        h,
        now,
        _shutdown: tx,
    }
}

async fn status(t: &TestApp) -> Value {
    let (code, _, body) = send(&t.app, get("/api/v1/status")).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    body["data"].clone()
}

fn codes(d: &Value) -> Vec<String> {
    d["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn disabled_capture_is_reported_as_disabled() {
    let t = app();
    let d = status(&t).await;
    assert_eq!(d["capture"], serde_json::json!({ "enabled": false }));
    assert!(!codes(&d).iter().any(|c| c.starts_with("capture_")));
}

#[tokio::test]
async fn enabled_capture_has_the_contract_shape() {
    let r = rig("127.0.0.1:0");
    let now = r.now.load(Ordering::SeqCst);
    r.h.health.datagram(now, 1518);
    inc(&r.h.health.counters.tls_named);
    inc(&r.h.health.counters.tls_ech);
    inc(&r.h.health.counters.dns_bindings);
    inc(&r.h.health.counters.unsupported_l3);
    inc(&r.h.health.counters.malformed_frame);
    r.h.health.record(now, |b| {
        b.lookups = 4;
        b.lookup_hits = 3;
    });
    let d = status(&r.t).await;
    let c = &d["capture"];
    assert_eq!(c["enabled"], true);
    assert_eq!(c["listening"], true);
    assert!(
        c["address"].as_str().unwrap().starts_with("127.0.0.1:"),
        "{c}"
    );
    assert!(c["error"].is_null());
    assert!(c["last_datagram_at"].is_string(), "{c}");
    for (group, keys) in [
        (
            "datagrams",
            &[
                "received",
                "rejected_source",
                "dropped_backpressure",
                "unsupported",
                "malformed",
                "max_len_15m",
            ][..],
        ),
        (
            "connections",
            &["tracked", "evicted", "reassembly_bytes"][..],
        ),
        ("bindings", &["tracked", "evicted"][..]),
        ("names", &["tls", "quic", "http", "dns", "ech"][..]),
        (
            "errors",
            &[
                "tls_incomplete",
                "tls_parse",
                "quic_unsupported_version",
                "quic_decrypt",
                "quic_incomplete",
                "dns_parse",
                "name_invalid",
            ][..],
        ),
    ] {
        for k in keys {
            assert!(c[group][k].is_u64(), "capture.{group}.{k} missing in {c}");
        }
    }
    assert_eq!(c["datagrams"]["received"], 1);
    assert_eq!(c["datagrams"]["unsupported"], 1);
    assert_eq!(c["datagrams"]["malformed"], 1);
    assert_eq!(c["datagrams"]["max_len_15m"], 1518);
    assert_eq!(c["names"]["tls"], 1);
    assert_eq!(c["names"]["ech"], 1);
    assert_eq!(c["names"]["dns"], 1);
    assert_eq!(c["named_share_15m"], 0.75);
    assert!(codes(&d).iter().all(|c| !c.starts_with("capture_")), "{d}");
}

#[tokio::test]
async fn named_share_is_null_without_lookups() {
    let r = rig("127.0.0.1:0");
    assert!(status(&r.t).await["capture"]["named_share_15m"].is_null());
}

#[tokio::test]
async fn detector_warnings_reach_status() {
    let r = rig("127.0.0.1:0");
    let now = r.now.load(Ordering::SeqCst);
    r.h.health.record(now, |b| {
        b.tls_started = 60;
        b.tls_incomplete = 60;
        b.max_datagram_len = 1200;
        b.tcp_syn = 120;
    });
    // Nothing has arrived for longer than silent_after.
    r.now.store(
        now + r.h.settings.silent_after.as_millis() as i64 + 1_000,
        Ordering::SeqCst,
    );
    let d = status(&r.t).await;
    let codes = codes(&d);
    for code in ["capture_silent", "capture_fastpath_suspected"] {
        assert!(
            codes.contains(&code.to_string()),
            "{code} missing in {codes:?}"
        );
    }
    // The 15-min window is still within reach after 5 min.
    assert!(
        codes.contains(&"capture_large_packets_lost".to_string()),
        "{codes:?}"
    );
    let w = d["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["code"] == "capture_large_packets_lost")
        .unwrap();
    assert!(w["message"].as_str().unwrap().contains("WSL2"));
}

#[tokio::test]
async fn bind_failure_is_a_collector_down_warning() {
    // TEST-NET-3 is never a local address, so the bind fails.
    let r = rig("203.0.113.1:37008");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let d = status(&r.t).await;
    assert_eq!(d["capture"]["listening"], false);
    assert!(d["capture"]["error"].is_string());
    assert!(
        codes(&d).contains(&"collector_down:37008".to_string()),
        "{d}"
    );
}

#[tokio::test]
async fn metrics_export_every_capture_series() {
    let r = rig("127.0.0.1:0");
    inc(&r.h.health.counters.quic_named);
    let req = axum::http::Request::get("/metrics")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = tower::ServiceExt::oneshot(r.t.app.clone(), req)
        .await
        .unwrap();
    let text = String::from_utf8(
        http_body_util::BodyExt::collect(res.into_body())
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    for needle in [
        "# TYPE tmon_capture_datagrams_total counter",
        "tmon_capture_datagrams_total 0",
        "tmon_capture_rejected_total 0",
        "tmon_capture_dropped_total{reason=\"backpressure\"} 0",
        "tmon_capture_unsupported_total{kind=\"tzsp\"}",
        "tmon_capture_unsupported_total{kind=\"encap\"}",
        "tmon_capture_unsupported_total{kind=\"l3\"}",
        "tmon_capture_unsupported_total{kind=\"fragment\"}",
        "tmon_capture_malformed_total{layer=\"tzsp\"}",
        "tmon_capture_malformed_total{layer=\"frame\"}",
        "tmon_capture_names_total{source=\"tls\"} 0",
        "tmon_capture_names_total{source=\"quic\"} 1",
        "tmon_capture_names_total{source=\"http\"}",
        "tmon_capture_names_total{source=\"dns\"}",
        "tmon_capture_errors_total{kind=\"tls_incomplete\"}",
        "tmon_capture_errors_total{kind=\"tls_parse\"}",
        "tmon_capture_errors_total{kind=\"quic_version\"}",
        "tmon_capture_errors_total{kind=\"quic_decrypt\"}",
        "tmon_capture_errors_total{kind=\"quic_incomplete\"}",
        "tmon_capture_errors_total{kind=\"dns_parse\"}",
        "tmon_capture_errors_total{kind=\"name_invalid\"}",
        "tmon_capture_evicted_total{what=\"conn_lru\"}",
        "tmon_capture_evicted_total{what=\"conn_ttl\"}",
        "tmon_capture_evicted_total{what=\"conn_bytes\"}",
        "tmon_capture_evicted_total{what=\"binding\"}",
        "# TYPE tmon_capture_connections gauge",
        "# TYPE tmon_capture_bindings gauge",
        "# TYPE tmon_capture_reassembly_bytes gauge",
        "tmon_capture_lookups_total{result=\"conn\"}",
        "tmon_capture_lookups_total{result=\"dns\"}",
        "tmon_capture_lookups_total{result=\"miss\"}",
    ] {
        assert!(text.contains(needle), "missing `{needle}` in\n{text}");
    }
}

#[tokio::test]
async fn metrics_omit_capture_series_when_disabled() {
    let t = app();
    let req = axum::http::Request::get("/metrics")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = tower::ServiceExt::oneshot(t.app.clone(), req)
        .await
        .unwrap();
    let text = String::from_utf8(
        http_body_util::BodyExt::collect(res.into_body())
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(!text.contains("tmon_capture_"));
}
