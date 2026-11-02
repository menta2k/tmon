mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tmon_collector::exporter_registry::{CountDirection, FlowProtocol};
use tmon_core::RawFlow;

fn raw(exporter_id: u16, flow_direction: Option<u8>) -> RawFlow {
    RawFlow {
        ts_ms: 1,
        ts_start_ms: 0,
        exporter_id,
        in_if: 0,
        out_if: 0,
        src_ip: "1.1.1.1".parse().unwrap(),
        dst_ip: "192.168.88.10".parse().unwrap(),
        src_port: 443,
        dst_port: 40000,
        proto: 6,
        tcp_flags: 0,
        bytes: 100,
        packets: 1,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction,
    }
}

#[tokio::test]
async fn patch_updates_registry_and_persists() {
    let t = app();
    let (e, _) = t
        .state
        .registry
        .get_or_create("192.168.88.1".parse().unwrap(), 0, FlowProtocol::Ipfix, 0)
        .unwrap();
    let body =
        json!({"name": "core router", "sampling_override": 10, "count_direction": "ingress"});
    let (status, _, res) = send(
        &t.app,
        json_req("PATCH", &format!("/api/v1/exporters/{}", e.id), body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(res["data"]["name"], "core router");
    assert_eq!(res["data"]["sampling_rate"], 10);
    assert_eq!(e.settings().count_direction, CountDirection::Ingress);
    assert_eq!(e.sampling_rate(), 10);
    let rows = tmon_store::duck::repo::exporters::list(&t.state.store.conn().unwrap()).unwrap();
    assert_eq!(rows[0].name.as_deref(), Some("core router"));

    // ingress-only: egress-reported (flowDirection=1) duplicates are dropped
    let out = t
        .state
        .pipeline
        .process(&[raw(e.id, Some(0)), raw(e.id, Some(1)), raw(e.id, None)]);
    assert_eq!(out.len(), 2);

    let (status, _, _) = send(
        &t.app,
        json_req(
            "PATCH",
            &format!("/api/v1/exporters/{}", e.id),
            json!({"sampling_override": null}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(e.sampling_rate(), 1);
}

#[tokio::test]
async fn patch_validates_input() {
    let t = app();
    let (e, _) = t
        .state
        .registry
        .get_or_create("192.168.88.1".parse().unwrap(), 0, FlowProtocol::Ipfix, 0)
        .unwrap();
    let uri = format!("/api/v1/exporters/{}", e.id);
    let (status, _, body) = send(
        &t.app,
        json_req(
            "PATCH",
            &uri,
            json!({"sampling_override": 0, "count_direction": "sideways"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"]["fields"]["sampling_override"].is_string());
    assert!(body["error"]["fields"]["count_direction"].is_string());
    let (status, _, _) = send(&t.app, json_req("PATCH", &uri, json!({"bogus": 1}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = send(
        &t.app,
        json_req("PATCH", "/api/v1/exporters/999", json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
