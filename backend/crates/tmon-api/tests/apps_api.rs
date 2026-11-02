//! Application classification over HTTP (feature 003, US2/US3):
//! `app` filter, `by=app`, host `top_apps`, catalog, `/status.dpi`, warnings.

mod common;

use axum::http::StatusCode;
use common::*;
use std::sync::Arc;
use tmon_api::router;
use tmon_core::app::AppInfo;
use tmon_core::{Direction, FlowRecord, RawFlow};
use tmon_dpi_client::{DpiHandle, DpiSettings, DpiState};
use tmon_dpi_proto::{CatalogEntry, Hello};
use tmon_store::FlowStore;

fn rec(ts: i64, local: &str, port: u16, bytes: u64) -> FlowRecord {
    let raw = RawFlow {
        ts_ms: ts,
        ts_start_ms: ts - 1000,
        exporter_id: 1,
        in_if: 0,
        out_if: 0,
        src_ip: local.parse().unwrap(),
        dst_ip: "203.0.113.5".parse().unwrap(),
        src_port: 40000,
        dst_port: port,
        proto: 6,
        tcp_flags: 0,
        bytes,
        packets: 1,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    };
    FlowRecord::from_raw(&raw, Direction::Up, true).with_category(1, None)
}

fn with_app(f: FlowRecord, app: &str, confident: bool) -> FlowRecord {
    f.with_app(&AppInfo::new(app, "Download", confident, None, false).unwrap())
}

fn seeded() -> TestApp {
    let t = app();
    let now = t.state.now_ms();
    t.state
        .store
        .append_batch(&[
            with_app(
                rec(now - 1_000, "192.168.88.10", 6881, 5_000),
                "BitTorrent",
                true,
            ),
            with_app(
                rec(now - 2_000, "192.168.88.10", 6881, 3_000),
                "BitTorrent",
                false,
            ),
            with_app(
                rec(now - 3_000, "192.168.88.11", 51820, 2_000),
                "WireGuard",
                false,
            ),
            rec(now - 4_000, "192.168.88.10", 443, 1_000),
        ])
        .unwrap();
    tmon_store::duck::repo::hosts::touch(
        &t.state.store.conn().unwrap(),
        &[("192.168.88.10".into(), now - 4_000, now)],
    )
    .unwrap();
    t
}

fn hello() -> Hello {
    let e = |a: &str, c: &str| CatalogEntry {
        app: a.into(),
        category: c.into(),
    };
    Hello {
        proto_version: 1,
        impl_name: "fake".into(),
        impl_version: "0.3.0".into(),
        engine: "nDPI 6.0".into(),
        max_flows: 1_000,
        catalog: vec![
            e("WireGuard", "VPN"),
            e("BitTorrent", "Download"),
            e("bittorrent", "Download"),
        ],
    }
}

/// The test app with a classifier handle in `state`.
fn with_dpi(t: &TestApp) -> (axum::Router, Arc<DpiHandle>) {
    let (h, _rx) = DpiHandle::new(DpiSettings::default());
    let h = Arc::new(h);
    let state = tmon_api::AppState {
        dpi: Some(Arc::clone(&h)),
        ..t.state.clone()
    };
    (router(state), h)
}

#[tokio::test]
async fn flows_filter_by_app_and_carry_app_fields() {
    let t = seeded();
    let (s, _, b) = send(&t.app, get("/api/v1/flows?from=now-5m&app=bittorrent")).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["meta"]["total"], 2);
    let rows = b["data"].as_array().unwrap();
    assert!(rows.iter().all(|r| r["app"] == "BitTorrent"));
    assert_eq!(rows[0]["app_confident"], true);
    let (_, _, all) = send(&t.app, get("/api/v1/flows?from=now-5m")).await;
    let plain = all["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["bytes"] == 1_000)
        .unwrap()
        .clone();
    assert!(plain["app"].is_null() && plain["app_confident"].is_null());
    let long = "a".repeat(49);
    let (s, _, _) = send(&t.app, get(&format!("/api/v1/flows?app={long}"))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, b) = send(&t.app, get("/api/v1/flows?from=now-30d&app=BitTorrent")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{b}");
}

#[tokio::test]
async fn empty_app_filters_unclassified_flows() {
    let t = seeded();
    let (s, _, b) = send(&t.app, get("/api/v1/flows?from=now-5m&app=")).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["meta"]["total"], 1);
    assert!(b["data"][0]["app"].is_null());
    let (_, _, b) = send(&t.app, get("/api/v1/flows?from=now-5m&app=&app=WireGuard")).await;
    assert_eq!(b["meta"]["total"], 2);
    let (s, _, _) = send(&t.app, get("/api/v1/flows?from=now-5m&app=%3Cx%3E")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn breakdown_by_app_and_host_top_apps() {
    let t = seeded();
    let (s, _, b) = send(&t.app, get("/api/v1/traffic/breakdown?by=app&from=now-5m")).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let rows = b["data"].as_array().unwrap();
    let bt = rows.iter().find(|r| r["key"] == "BitTorrent").unwrap();
    assert_eq!(
        (bt["bytes"].as_u64(), bt["flows"].as_u64()),
        (Some(8_000), Some(2))
    );
    assert_eq!(bt["confident_share"], 0.5);
    assert!(
        rows.iter()
            .any(|r| r["key"] == "" && r["label"] == "Unclassified")
    );
    let (s, _, _) = send(&t.app, get("/api/v1/traffic/breakdown?by=app&from=now-30d")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    let (s, _, h) = send(&t.app, get("/api/v1/hosts/192.168.88.10?from=now-5m")).await;
    assert_eq!(s, StatusCode::OK, "{h}");
    let top = h["data"]["top_apps"].as_array().unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0]["app"], "BitTorrent");
    assert_eq!(top[0]["bytes"], 8_000);
}

#[tokio::test]
async fn catalog_is_empty_without_dpi_and_sorted_with_it() {
    let t = app();
    let (s, _, b) = send(&t.app, get("/api/v1/applications/catalog")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["data"], serde_json::json!([]));

    let (app, h) = with_dpi(&t);
    h.health.running(&hello(), t.state.now_ms());
    let (_, _, b) = send(&app, get("/api/v1/applications/catalog")).await;
    assert_eq!(
        b["data"],
        serde_json::json!([
            {"app": "BitTorrent", "category": "Download"},
            {"app": "WireGuard", "category": "VPN"}
        ])
    );
}

#[tokio::test]
async fn status_dpi_is_always_present() {
    let t = app();
    let (_, _, b) = send(&t.app, get("/api/v1/status")).await;
    assert_eq!(b["data"]["dpi"], serde_json::json!({"enabled": false}));

    let (app, h) = with_dpi(&t);
    h.health.running(&hello(), t.state.now_ms());
    let (_, _, b) = send(&app, get("/api/v1/status")).await;
    let dpi = &b["data"]["dpi"];
    assert_eq!(dpi["enabled"], true);
    assert_eq!(dpi["state"], "running");
    assert_eq!(dpi["engine"], "nDPI 6.0");
    assert_eq!(dpi["proto_version"], 1);
    for k in [
        "sent",
        "dropped_queue",
        "dropped_down",
        "skipped_final",
        "drop_share_15m",
    ] {
        assert!(dpi["queue"].get(k).is_some(), "queue.{k}");
    }
    for k in [
        "packets_in",
        "flows_active",
        "classified_confident",
        "classified_guess",
        "evicted",
        "mem_bytes",
    ] {
        assert!(dpi["classifier"].get(k).is_some(), "classifier.{k}");
    }
    assert!(dpi["top_apps"].is_array());
}

#[tokio::test]
async fn classifier_problems_become_warnings() {
    let t = app();
    let (app, h) = with_dpi(&t);
    h.health.set_state(DpiState::NotFound, None);
    let (_, _, b) = send(&app, get("/api/v1/status")).await;
    let codes: Vec<&str> = b["data"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"dpi_not_found"), "{codes:?}");
    assert_eq!(b["data"]["dpi"]["state"], "not_found");

    let now = t.state.now_ms();
    h.health.set_state(DpiState::Restarting, None);
    h.health.failure("exit status 139".into(), now - 1_000);
    h.health.failure("exit status 139".into(), now);
    let (_, _, b) = send(&app, get("/api/v1/status")).await;
    let w = b["data"]["warnings"].as_array().unwrap();
    assert!(w.iter().any(|w| w["code"] == "dpi_restarting"), "{w:?}");
}

async fn metrics_text(app: &axum::Router) -> String {
    let req = axum::http::Request::get("/metrics")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
    let body = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap();
    String::from_utf8(body.to_bytes().to_vec()).unwrap()
}

#[tokio::test]
async fn metrics_expose_bounded_dpi_families_only_when_enabled() {
    let t = app();
    assert!(!metrics_text(&t.app).await.contains("tmon_dpi_"));
    let (app, h) = with_dpi(&t);
    h.health.running(&hello(), t.state.now_ms());
    let text = metrics_text(&app).await;
    for needle in [
        "tmon_dpi_up 1",
        "tmon_dpi_restarts_total 0",
        "tmon_dpi_packets_total{result=\"sent\"} 0",
        "tmon_dpi_packets_total{result=\"dropped_queue\"} 0",
        "tmon_dpi_classified_total{confidence=\"guess\"} 0",
        "# TYPE tmon_dpi_flows_active gauge",
        "# TYPE tmon_dpi_memory_bytes gauge",
        "tmon_dpi_evicted_total{reason=\"memory\"} 0",
        "tmon_dpi_ipc_invalid_total 0",
    ] {
        assert!(text.contains(needle), "missing `{needle}` in\n{text}");
    }
    assert!(!text.contains("BitTorrent"), "no per-app labels");
}
