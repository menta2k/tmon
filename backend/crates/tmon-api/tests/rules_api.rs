mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tmon_core::RawFlow;

fn game_flow() -> RawFlow {
    RawFlow {
        ts_ms: 1,
        ts_start_ms: 0,
        exporter_id: 0,
        in_if: 0,
        out_if: 0,
        src_ip: "203.0.113.50".parse().unwrap(),
        dst_ip: "192.168.88.11".parse().unwrap(),
        src_port: 40150,
        dst_port: 50000,
        proto: 17,
        tcp_flags: 0,
        bytes: 1000,
        packets: 1,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    }
}

async fn gaming_id(t: &TestApp) -> u64 {
    let (_, _, cats) = send(&t.app, get("/api/v1/categories")).await;
    cats["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Gaming")
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}

#[tokio::test]
async fn category_crud_and_validation() {
    let t = app();
    let (s, _, c) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/categories",
            json!({"name": "IoT", "color": "#123456", "description": "smart home"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{c}");
    let id = c["data"]["id"].as_u64().unwrap();
    assert_eq!(c["data"]["builtin"], false);

    let (s, _, bad) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/categories",
            json!({"name": "", "color": "red"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(bad["error"]["fields"]["name"].is_string());
    assert!(bad["error"]["fields"]["color"].is_string());
    let (s, _, _) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/categories",
            json!({"name": "iot", "color": "#000000"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "duplicate names are rejected");

    let (s, _, u) = send(
        &t.app,
        json_req(
            "PATCH",
            &format!("/api/v1/categories/{id}"),
            json!({"color": "#abcdef"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(u["data"]["color"], "#abcdef");
    assert_eq!(u["data"]["name"], "IoT");

    let (s, _, _) = send(
        &t.app,
        json_req("DELETE", &format!("/api/v1/categories/{id}"), json!(null)),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    for protected in [0, gaming_id(&t).await] {
        let (s, _, b) = send(
            &t.app,
            json_req(
                "DELETE",
                &format!("/api/v1/categories/{protected}"),
                json!(null),
            ),
        )
        .await;
        assert_eq!(s, StatusCode::CONFLICT);
        assert_eq!(b["error"]["code"], "conflict");
    }
    let (s, _, _) = send(
        &t.app,
        json_req("PATCH", "/api/v1/categories/999", json!({"name": "x"})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rule_crud_validation_and_live_apply() {
    let t = app();
    let gaming = gaming_id(&t).await;
    let before = t.state.pipeline.enrich(&game_flow());
    assert_ne!(u64::from(before.category_id), gaming);

    let (s, _, bad) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/rules",
            json!({"name": "x", "category_id": gaming, "match": {"remote_cidr": ["nope"]}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(
        bad["error"]["fields"]["match.remote_cidr[0]"],
        "invalid CIDR"
    );
    let (s, _, bad) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/rules",
            json!({"name": "x", "category_id": 4242, "match": {"proto": [17]}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");

    let body = json!({"name": "Steam", "category_id": gaming, "enabled": true, "match": {"proto": [17], "remote_port": ["40100-40200"]}});
    let (s, _, r) = send(&t.app, json_req("POST", "/api/v1/rules", body)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let id = r["data"]["id"].as_u64().unwrap();
    assert_eq!(r["data"]["origin"], "user");
    assert!(r["data"]["priority"].as_i64().unwrap() >= 1_000_000);

    let after = t.state.pipeline.enrich(&game_flow());
    assert_eq!(
        u64::from(after.category_id),
        gaming,
        "new rule applies to new flows"
    );
    assert_eq!(after.rule_id.map(u64::from), Some(id));

    let (_, _, list) = send(&t.app, get("/api/v1/rules")).await;
    let mine = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(mine["hits_24h"], 0);
    assert_eq!(mine["match"]["remote_port"][0], "40100-40200");

    let (s, _, p) = send(
        &t.app,
        json_req(
            "PATCH",
            &format!("/api/v1/rules/{id}"),
            json!({"enabled": false}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_ne!(
        u64::from(t.state.pipeline.enrich(&game_flow()).category_id),
        gaming
    );

    let builtin = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["origin"] == "builtin")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let (s, _, _) = send(
        &t.app,
        json_req("DELETE", &format!("/api/v1/rules/{builtin}"), json!(null)),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _, _) = send(
        &t.app,
        json_req(
            "PATCH",
            &format!("/api/v1/rules/{builtin}"),
            json!({"enabled": false}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "built-ins can be disabled");
    let (s, _, _) = send(
        &t.app,
        json_req("DELETE", &format!("/api/v1/rules/{id}"), json!(null)),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn reorder_assigns_user_priorities_in_order() {
    let t = app();
    let gaming = gaming_id(&t).await;
    let mut ids = Vec::new();
    for port in [1000, 2000, 3000] {
        let (_, _, r) = send(&t.app, json_req("POST", "/api/v1/rules", json!({"name": format!("p{port}"), "category_id": gaming, "match": {"remote_port": [port]}}))).await;
        ids.push(r["data"]["id"].as_u64().unwrap());
    }
    let order = vec![ids[2], ids[0], ids[1]];
    let (s, _, b) = send(
        &t.app,
        json_req("POST", "/api/v1/rules/reorder", json!({"ids": order})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (_, _, list) = send(&t.app, get("/api/v1/rules")).await;
    let users: Vec<u64> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["origin"] == "user")
        .map(|r| r["id"].as_u64().unwrap())
        .collect();
    assert_eq!(users, order, "GET /rules is ordered by priority");
    let (s, _, _) = send(
        &t.app,
        json_req("POST", "/api/v1/rules/reorder", json!({"ids": [ids[0]]})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "partial lists are rejected");
}

#[tokio::test]
async fn rule_test_dry_run() {
    let t = app();
    let (s, _, r) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/rules/test",
            json!({"flow": {"proto": 17, "remote_port": 53, "direction": "up"}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (_, _, cats) = send(&t.app, get("/api/v1/categories")).await;
    let dns = cats["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "DNS")
        .unwrap()["id"]
        .clone();
    assert_eq!(r["data"]["category_id"], dns);
    assert!(r["data"]["rule_id"].is_u64());
    let (s, _, _) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/rules/test",
            json!({"flow": {"remote_ip": "nope"}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn reapply_runs_as_a_job() {
    let t = app();
    let (s, _, r) = send(
        &t.app,
        json_req("POST", "/api/v1/rules/reapply", json!({"from": "now-1h"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let id = r["data"]["job_id"].as_u64().unwrap();
    let mut state = String::new();
    for _ in 0..100 {
        let (_, _, j) = send(&t.app, get(&format!("/api/v1/jobs/{id}"))).await;
        state = j["data"]["state"].as_str().unwrap().to_string();
        if state == "done" || state == "failed" {
            assert_eq!(j["data"]["progress"], 1.0);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(state, "done");
    let (s, _, _) = send(&t.app, get("/api/v1/jobs/9999")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn app_rules_round_trip_and_test_reports_confidence() {
    let t = app();
    let (_, _, cats) = send(&t.app, get("/api/v1/categories")).await;
    let id = |name: &str| {
        cats["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap()["id"]
            .as_u64()
            .unwrap()
    };
    let (s, _, r) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/rules",
            json!({"name": "Zoom is work", "category_id": id("Web"), "match": {"app": ["Zoom"], "app_category": ["Video"]}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["data"]["match"]["app"], json!(["Zoom"]));
    let test =
        |flow: serde_json::Value| json_req("POST", "/api/v1/rules/test", json!({ "flow": flow }));
    let (s, _, r) = send(
        &t.app,
        test(json!({"remote_port": 443, "app": "Zoom", "app_category": "Video", "app_confident": true})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["data"]["category_id"], id("Web"));
    assert_eq!(r["data"]["confidence"], "detected");
    let (_, _, r) = send(
        &t.app,
        test(json!({"proto": 6, "remote_port": 6881, "app": "BitTorrent", "app_category": "Download", "app_confident": false})),
    )
    .await;
    assert_eq!(r["data"]["category_id"], id("File Sharing"));
    assert_eq!(r["data"]["confidence"], "inferred");
    let (s, _, _) = send(&t.app, test(json!({"app": "TLS"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = send(
        &t.app,
        json_req(
            "POST",
            "/api/v1/rules",
            json!({"name": "bad", "category_id": 1, "match": {"app": []}}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
