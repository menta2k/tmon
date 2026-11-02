mod common;

use axum::http::StatusCode;
use common::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tmon_collector::exporter_registry::FlowProtocol;

#[tokio::test]
async fn status_matches_contract_shape_and_reports_stall() {
    let now = Arc::new(AtomicI64::new(1_000_000));
    let clock_now = now.clone();
    let t = app_with_clock(Arc::new(move || clock_now.load(Ordering::SeqCst)));
    let (e, _) = t
        .state
        .registry
        .get_or_create(
            "192.168.88.1".parse().unwrap(),
            0,
            FlowProtocol::Ipfix,
            1_000_000,
        )
        .unwrap();
    e.counters.add_received();
    e.counters.add_decoded(10, 5000);

    let (status, _, body) = send(&t.app, get("/api/v1/status")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let d = &body["data"];
    let ex = &d["exporters"][0];
    for k in [
        "id",
        "address",
        "protocol",
        "flows_per_sec",
        "last_seen",
        "sampling_rate",
        "received",
        "decoded",
        "template_missing",
        "parse_errors",
        "dropped",
        "state",
    ] {
        assert!(!ex[k].is_null(), "exporter.{k} missing: {ex}");
    }
    assert_eq!(ex["state"], "active");
    assert_eq!(ex["protocol"], "ipfix");
    assert!(d["collectors"].is_array());
    assert!(d["ingest"]["flows_per_sec"].is_number());
    assert!(d["ingest"]["dropped_total"].is_object());
    assert!(d["storage"]["disk_bytes"].as_u64().unwrap() > 0);
    assert!(d["storage"]["cap_bytes"].as_u64().unwrap() > 0);
    assert!(d["enrichment"]["router_integration"]["enabled"].is_boolean());
    let no_stall = d["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .all(|w| !w["code"].as_str().unwrap().starts_with("exporter_stalled"));
    assert!(no_stall);

    now.store(1_000_000 + 6 * 60_000, Ordering::SeqCst);
    let (_, _, body) = send(&t.app, get("/api/v1/status")).await;
    assert_eq!(body["data"]["exporters"][0]["state"], "stalled");
    let msgs: Vec<&str> = body["data"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["message"].as_str().unwrap())
        .collect();
    assert!(
        msgs.contains(&"No flows received from 192.168.88.1 for 6 min"),
        "{msgs:?}"
    );
}

#[tokio::test]
async fn metrics_exposes_bounded_prometheus_families() {
    let t = app();
    let (e, _) = t
        .state
        .registry
        .get_or_create(
            "192.168.88.1".parse().unwrap(),
            0,
            FlowProtocol::Netflow9,
            0,
        )
        .unwrap();
    e.counters.add_decoded(3, 300);
    t.state
        .globals
        .drop_n(tmon_collector::DropReason::Backpressure, 2);
    let req = axum::http::Request::get("/metrics")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = tower::ServiceExt::oneshot(t.app.clone(), req)
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );
    let text = String::from_utf8(
        http_body_util::BodyExt::collect(res.into_body())
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    for needle in [
        "tmon_flows_received_total{exporter=\"192.168.88.1\",proto=\"netflow9\"} 3",
        "tmon_flows_dropped_total{reason=\"backpressure\"} 2",
        "# TYPE tmon_decode_errors_total counter",
        "tmon_template_missing_total 0",
        "# TYPE tmon_bytes_total counter",
        "# TYPE tmon_storage_bytes gauge",
    ] {
        assert!(text.contains(needle), "missing `{needle}` in\n{text}");
    }
    let anon = axum::http::Request::get("/metrics")
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(
        tower::ServiceExt::oneshot(t.app.clone(), anon)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
