//! US1: replay a mixed home capture and check the dashboard API (quickstart V2/V4/V5).

mod common;

use common::*;
use tmon_collector::listener::ListenerKind;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn category_breakdown_matches_ground_truth() {
    let it = start().await;
    let truth = truth_by("mixed-home.truth.csv", 0);
    let total: u64 = truth.values().sum();
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    let body = it
        .wait_for_bytes(
            "/traffic/breakdown?by=category&from=now-15m&to=now%2B1m&limit=100",
            total,
        )
        .await;

    assert_eq!(body["ok"], true);
    assert!(["rollup_1m", "rollup_1h", "raw"].contains(&body["meta"]["source"].as_str().unwrap()));
    let rows = body["data"].as_array().unwrap();
    for (cat, expected) in &truth {
        let got = rows
            .iter()
            .find(|r| r["label"] == cat.as_str())
            .and_then(|r| r["bytes"].as_u64())
            .unwrap_or(0);
        assert!(
            within(got, *expected, 1.0),
            "{cat}: got {got}, expected {expected}"
        );
    }
    let other = rows
        .iter()
        .find(|r| r["key"] == "0")
        .and_then(|r| r["share"].as_f64())
        .unwrap_or(0.0);
    assert!(other <= 0.20, "Other share {other} > 20 % (SC-006)");
    assert_eq!(body["meta"]["sampled"], false);
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direction_timeseries_has_step_and_series() {
    let it = start().await;
    let truth = truth_by("mixed-home.truth.csv", 1);
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    it.wait_for_bytes(
        "/traffic/breakdown?by=direction&from=now-15m&to=now%2B1m",
        truth.values().sum(),
    )
    .await;

    let (status, body) = it
        .get("/traffic/timeseries?group_by=direction&from=now-15m&to=now%2B1m")
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["data"]["step"].as_i64().unwrap() >= 60);
    let series = body["data"]["series"].as_array().unwrap();
    let down = series
        .iter()
        .find(|s| s["key"] == "down")
        .expect("down series");
    assert_eq!(down["label"], "Download");
    let sum: u64 = down["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p[1].as_u64().unwrap())
        .sum();
    assert!(
        within(sum, truth["down"], 1.0),
        "down {sum} vs {}",
        truth["down"]
    );
    assert_eq!(body["meta"]["source"], "rollup_1m");
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sampled_sflow_sets_meta_sampled() {
    let it = start().await;
    it.replay("sflow-crs-1in512.pcap", ListenerKind::Sflow)
        .await;
    let body = it
        .wait_for_bytes("/traffic/breakdown?by=category&from=now-15m&to=now%2B1m", 1)
        .await;
    assert_eq!(body["meta"]["sampled"], true);
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn breakdown_by_rule_reports_applications_with_confidence() {
    let it = start().await;
    let total: u64 = truth_by("mixed-home.truth.csv", 0).values().sum();
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    let body = it
        .wait_for_bytes(
            "/traffic/breakdown?by=rule&from=now-15m&to=now%2B1m&limit=100",
            total,
        )
        .await;
    let rows = body["data"].as_array().unwrap();
    let web = rows
        .iter()
        .find(|r| r["label"] == "HTTPS / HTTP / QUIC")
        .expect("web catch-all row");
    assert_eq!(web["confidence"], "guessed");
    let dns = rows.iter().find(|r| r["label"] == "DNS").expect("dns row");
    assert_eq!(dns["confidence"], "guessed");
    assert!(rows.iter().all(|r| {
        ["detected", "inferred", "guessed", "none"].contains(&r["confidence"].as_str().unwrap())
    }));
    let (_, cats) = it
        .get("/traffic/breakdown?by=category&from=now-15m&to=now%2B1m")
        .await;
    assert!(
        cats["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r.get("confidence").is_none()),
        "only by=rule has confidence"
    );
    it.app.shutdown().await;
}
