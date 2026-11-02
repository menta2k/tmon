//! US4: router DHCP/DNS enrichment (quickstart V9).

mod common;

use common::*;
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;
use tmon_collector::listener::ListenerKind;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RANGE: &str = "from=now-15m&to=now%2B1m";

async fn mock_router() -> MockServer {
    // Not pooled: dropping it must really close the port (pooled servers stay alive).
    let server = MockServer::builder().start().await;
    let leases = json!([{"address": "192.168.88.10", "mac-address": "AA:BB:CC:00:00:10", "host-name": "laptop"}]);
    let dns = json!([{"name": "cdn.example-video.com", "data": "203.0.113.10", "type": "A"}]);
    Mock::given(method("GET"))
        .and(path("/rest/ip/dhcp-server/lease"))
        .respond_with(ResponseTemplate::new(200).set_body_json(leases))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/ip/dns/cache"))
        .respond_with(ResponseTemplate::new(200).set_body_json(dns))
        .mount(&server)
        .await;
    server
}

async fn wait_status(it: &It, pred: impl Fn(&Value) -> bool) -> Value {
    let mut last = Value::Null;
    for _ in 0..150 {
        let (_, s) = it.get("/status").await;
        last = s["data"]["enrichment"].clone();
        let r = last["router_integration"].clone();
        if pred(&r) {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("router status condition not reached; last enrichment status: {last}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn router_names_enrich_hosts_remotes_and_rules() {
    let it = start().await;
    let router = mock_router().await;
    let patch = json!({"router": {"enabled": true, "url": router.uri(), "username": "tmon-ro", "password": "secret", "lease_poll_secs": 5, "dns_poll_secs": 5}});
    let (s, b) = it.send_json(Method::PATCH, "/settings", patch).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["data"]["router"]["password_set"], true);
    assert!(!b.to_string().contains("secret"));
    wait_status(&it, |r| r["last_ok"].is_i64()).await;

    let (_, cats) = it.get("/categories").await;
    let streaming = cats["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Streaming")
        .unwrap()["id"]
        .clone();
    // Breakdown keys are strings.
    let streaming_key = streaming.to_string();
    let (s, b) = it.send_json(Method::POST, "/rules", json!({"name": "video", "category_id": streaming, "match": {"hostname": ["*.example-video.com"]}})).await;
    assert_eq!(s, 200, "{b}");

    let total: u64 = truth_by("mixed-home.truth.csv", 0).values().sum();
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    let breakdown = it
        .wait_for_bytes(
            &format!("/traffic/breakdown?by=category&{RANGE}&limit=100"),
            total,
        )
        .await;
    let stream_bytes = breakdown["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == streaming_key)
        .and_then(|r| r["bytes"].as_u64())
        .unwrap_or(0);
    assert!(
        stream_bytes > 0,
        "hostname rule categorized new flows: {breakdown}"
    );

    let (_, remotes) = it.get(&format!("/top/remotes?{RANGE}&limit=50")).await;
    let cdn = remotes["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["ip"] == "203.0.113.10")
        .unwrap();
    assert_eq!(cdn["hostname"], "cdn.example-video.com");

    let (_, host) = it.get(&format!("/hosts/192.168.88.10?{RANGE}")).await;
    assert_eq!(host["data"]["host"]["dhcp_hostname"], "laptop");
    assert_eq!(host["data"]["host"]["mac"], "aa:bb:cc:00:00:10");
    assert_eq!(host["data"]["host"]["name"], "laptop");
    let (_, renamed) = it
        .send_json(
            Method::PATCH,
            "/hosts/192.168.88.10",
            json!({"friendly_name": "Dad's laptop"}),
        )
        .await;
    assert_eq!(
        renamed["data"]["name"], "Dad's laptop",
        "friendly name wins over DHCP"
    );

    drop(router);
    let r = wait_status(&it, |r| r["error"].is_string()).await;
    assert!(r["error"].as_str().unwrap().contains("unreachable"), "{r}");
    let (_, st) = it.get("/status").await;
    assert!(
        st["data"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "router_unreachable")
    );
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    it.wait_for_bytes(
        &format!("/traffic/breakdown?by=category&{RANGE}&limit=100"),
        total * 2,
    )
    .await;
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_learned_after_traffic_recategorize_recent_flows() {
    let it = start().await;
    let (_, cats) = it.get("/categories").await;
    let streaming = cats["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Streaming")
        .unwrap()["id"]
        .clone();
    // Breakdown keys are strings.
    let streaming_key = streaming.to_string();
    let (s, b) = it.send_json(Method::POST, "/rules", json!({"name": "video", "category_id": streaming, "match": {"hostname": ["*.example-video.com"]}})).await;
    assert_eq!(s, 200, "{b}");

    // Traffic first: no names known yet, so nothing is Streaming.
    let total: u64 = truth_by("mixed-home.truth.csv", 0).values().sum();
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    let url = format!("/traffic/breakdown?by=category&{RANGE}&limit=100");
    let before = it.wait_for_bytes(&url, total).await;
    let streaming_bytes = |v: &Value| {
        v["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["key"] == streaming_key)
            .and_then(|r| r["bytes"].as_u64())
            .unwrap_or(0)
    };
    assert_eq!(streaming_bytes(&before), 0, "{before}");

    // The DNS poll then learns 203.0.113.10 = cdn.example-video.com.
    let router = mock_router().await;
    let patch = json!({"router": {"enabled": true, "url": router.uri(), "username": "tmon-ro", "password": "secret", "lease_poll_secs": 5, "dns_poll_secs": 5}});
    let (s, b) = it.send_json(Method::PATCH, "/settings", patch).await;
    assert_eq!(s, 200, "{b}");
    let mut after = Value::Null;
    for _ in 0..100 {
        let (_, v) = it.get(&url).await;
        after = v;
        if streaming_bytes(&after) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        streaming_bytes(&after) > 0,
        "earlier flows moved to Streaming: {after}"
    );
    let sum: u64 = after["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["bytes"].as_u64())
        .sum();
    assert_eq!(
        sum, total,
        "back-fill moves bytes, never adds or loses them"
    );
    it.app.shutdown().await;
}
