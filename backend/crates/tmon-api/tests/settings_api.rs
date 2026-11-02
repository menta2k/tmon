mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tmon_core::{Direction, RawFlow};

#[tokio::test]
async fn host_patch_sets_name_and_mac() {
    let t = app();
    tmon_store::duck::repo::hosts::touch(
        &t.state.store.conn().unwrap(),
        &[("192.168.88.10".into(), 1, 2)],
    )
    .unwrap();
    let uri = "/api/v1/hosts/192.168.88.10";
    let (s, _, h) = send(
        &t.app,
        json_req(
            "PATCH",
            uri,
            json!({"friendly_name": "Living room TV", "mac": "AA-BB-CC-DD-EE-FF"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{h}");
    assert_eq!(h["data"]["name"], "Living room TV");
    assert_eq!(h["data"]["mac"], "aa:bb:cc:dd:ee:ff");
    let (s, _, bad) = send(&t.app, json_req("PATCH", uri, json!({"mac": "zz:zz"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(bad["error"]["fields"]["mac"].is_string());
    let (_, _, cleared) = send(
        &t.app,
        json_req("PATCH", uri, json!({"friendly_name": null})),
    )
    .await;
    assert_eq!(cleared["data"]["name"], "192.168.88.10");
    assert_eq!(
        cleared["data"]["mac"], "aa:bb:cc:dd:ee:ff",
        "absent fields are kept"
    );
    let (_, _, list) = send(&t.app, get("/api/v1/hosts?q=aa:bb")).await;
    assert_eq!(list["meta"]["total"], 1);
}

fn flow(src: &str, dst: &str) -> RawFlow {
    RawFlow {
        ts_ms: 1,
        ts_start_ms: 0,
        exporter_id: 0,
        in_if: 0,
        out_if: 0,
        src_ip: src.parse().unwrap(),
        dst_ip: dst.parse().unwrap(),
        src_port: 1,
        dst_port: 2,
        proto: 6,
        tcp_flags: 0,
        bytes: 1,
        packets: 1,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    }
}

#[tokio::test]
async fn networks_full_replace_validates_and_changes_direction() {
    let t = app();
    assert_eq!(
        t.state
            .pipeline
            .enrich(&flow("198.18.1.1", "8.8.8.8"))
            .direction,
        Direction::Transit
    );
    let (s, _, bad) = send(
        &t.app,
        json_req(
            "PUT",
            "/api/v1/networks",
            json!([{"cidr": "10.0.0.0/8"}, {"cidr": "bogus"}]),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(bad["error"]["fields"]["[1].cidr"].is_string());
    let (s, _, ok) = send(
        &t.app,
        json_req(
            "PUT",
            "/api/v1/networks",
            json!([{"cidr": "198.18.0.0/15", "label": "lab"}]),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{ok}");
    assert_eq!(ok["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        t.state
            .pipeline
            .enrich(&flow("198.18.1.1", "8.8.8.8"))
            .direction,
        Direction::Up
    );
}

#[tokio::test]
async fn settings_patch_and_export_has_no_secrets() {
    let t = app();
    let (s, _, v) = send(&t.app, get("/api/v1/settings")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["data"]["router"]["password_set"], true);
    assert!(!v.to_string().contains("router-secret"));
    let (s, _, p) = send(
        &t.app,
        json_req(
            "PATCH",
            "/api/v1/settings",
            json!({"retention": {"raw_days": 3}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(p["data"]["retention"]["raw_days"], 3);
    assert_eq!(t.state.windows().raw_ms, 3 * 86_400_000);
    let (s, _, _) = send(
        &t.app,
        json_req(
            "PATCH",
            "/api/v1/settings",
            json!({"retention": {"raw_days": 0}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, _, e) = send(&t.app, get("/api/v1/settings/export")).await;
    assert_eq!(s, StatusCode::OK);
    let text = e.to_string();
    assert!(
        !text.contains("router-secret") && !text.contains("password\""),
        "export leaks a secret: {text}"
    );
    assert_eq!(e["data"]["version"], 1);
    assert!(e["data"]["categories"].as_array().unwrap().len() >= 12);
}

#[tokio::test]
async fn import_is_validated_fully_then_applied_atomically() {
    let t = app();
    let (_, _, e) = send(&t.app, get("/api/v1/settings/export")).await;
    let mut bundle = e["data"].clone();
    let gaming = bundle["categories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Gaming")
        .unwrap()["id"]
        .clone();
    bundle["rules"].as_array_mut().unwrap().push(json!({"id": 20000, "category_id": gaming, "priority": 1_000_010, "enabled": true, "origin": "user", "name": "Steam", "match": {"remote_port": ["27015-27030"]}}));
    bundle["networks"] = json!([{"cidr": "10.9.0.0/16", "label": "lab"}]);
    bundle["hosts"] =
        json!([{"ip": "10.9.0.5", "mac": "aa:bb:cc:00:11:22", "friendly_name": "NAS"}]);

    let mut broken = bundle.clone();
    broken["rules"].as_array_mut().unwrap().push(json!({"id": 20001, "category_id": gaming, "priority": 1_000_020, "enabled": true, "origin": "user", "name": "bad", "match": {"remote_cidr": ["999.1.1.1/8"]}}));
    let (s, _, err) = send(&t.app, json_req("POST", "/api/v1/settings/import", broken)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let idx = bundle["rules"].as_array().unwrap().len();
    assert!(
        err["error"]["fields"][format!("rules[{idx}].match.remote_cidr[0]")].is_string(),
        "{err}"
    );
    let (_, _, nets) = send(&t.app, get("/api/v1/networks")).await;
    assert!(
        nets["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["cidr"] != "10.9.0.0/16"),
        "nothing applied"
    );

    let (s, _, ok) = send(&t.app, json_req("POST", "/api/v1/settings/import", bundle)).await;
    assert_eq!(s, StatusCode::OK, "{ok}");
    let (_, _, nets) = send(&t.app, get("/api/v1/networks")).await;
    assert_eq!(nets["data"][0]["cidr"], "10.9.0.0/16");
    let (_, _, rules) = send(&t.app, get("/api/v1/rules")).await;
    assert!(
        rules["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == 20000)
    );
    let (_, _, h) = send(&t.app, get("/api/v1/hosts/10.9.0.5?from=now-1h")).await;
    assert_eq!(h["data"]["host"]["name"], "NAS");
    let (_, _, st) = send(&t.app, get("/api/v1/settings")).await;
    assert_eq!(
        st["data"]["router"]["password_set"], true,
        "router password survives import"
    );
}

#[tokio::test]
async fn router_test_never_sends_stored_password_to_new_target() {
    let t = app();
    let body = json!({"url": "http://127.0.0.1:9", "username": "someone-else"});
    let (s, _, e) = send(
        &t.app,
        json_req("POST", "/api/v1/settings/router/test", body),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    assert!(e["error"]["fields"]["password"].is_string());
}

#[tokio::test]
async fn router_kind_is_patchable_and_validated() {
    let t = app();
    let (_, _, v) = send(&t.app, get("/api/v1/settings")).await;
    assert_eq!(v["data"]["router"]["kind"], "routeros");
    let patch = json!({"router": {"kind": "openwrt", "url": "http://192.168.1.1"}});
    let (s, _, p) = send(&t.app, json_req("PATCH", "/api/v1/settings", patch)).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(p["data"]["router"]["kind"], "openwrt");
    assert_eq!(p["data"]["router"]["password_set"], true, "password kept");
    let bad = json!({"router": {"kind": "pfsense"}});
    let (s, _, e) = send(&t.app, json_req("PATCH", "/api/v1/settings", bad)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    assert!(e["error"]["fields"]["router.kind"].is_string(), "{e}");
}

#[tokio::test]
async fn router_test_needs_password_to_disable_tls_checks() {
    let t = app();
    let body = json!({"verify_tls": false});
    let (s, _, e) = send(
        &t.app,
        json_req("POST", "/api/v1/settings/router/test", body),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    assert!(e["error"]["fields"]["password"].is_string());
}
