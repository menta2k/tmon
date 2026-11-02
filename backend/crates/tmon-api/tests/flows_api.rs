mod common;

use axum::http::StatusCode;
use common::*;
use http_body_util::BodyExt;
use tmon_core::{Direction, FlowRecord, RawFlow};
use tmon_store::FlowStore;
use tower::ServiceExt;

fn rec(ts: i64, remote: &str, port: u16) -> FlowRecord {
    rec_sized(ts, remote, port, 100)
}

fn rec_sized(ts: i64, remote: &str, port: u16, bytes: u64) -> FlowRecord {
    let raw = RawFlow {
        ts_ms: ts,
        ts_start_ms: ts - 1000,
        exporter_id: 1,
        in_if: 0,
        out_if: 0,
        src_ip: "192.168.88.10".parse().unwrap(),
        dst_ip: remote.parse().unwrap(),
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

fn seeded(n: usize) -> TestApp {
    let t = app();
    let now = t.state.now_ms();
    let flows: Vec<FlowRecord> = (0..n)
        .map(|i| {
            rec(
                now - 60_000 + i as i64 % 50_000,
                if i % 2 == 0 { "1.1.1.1" } else { "8.8.8.8" },
                if i % 3 == 0 { 443 } else { 80 },
            )
        })
        .collect();
    t.state.store.append_batch(&flows).unwrap();
    t
}

#[tokio::test]
async fn search_pages_newest_first_with_total() {
    let t = seeded(30);
    let (s, _, b) = send(&t.app, get("/api/v1/flows?from=now-5m&limit=10&page=2")).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["meta"]["total"], 30);
    assert_eq!(b["meta"]["page"], 2);
    let rows = b["data"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    let ts: Vec<i64> = rows.iter().map(|r| r["ts"].as_i64().unwrap()).collect();
    assert!(ts.windows(2).all(|w| w[0] >= w[1]), "newest first");
    assert_eq!(rows[0]["local_ip"], "192.168.88.10");
    assert_eq!(rows[0]["direction"], "up");
    let (s, _, _) = send(&t.app, get("/api/v1/flows?limit=501")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_filters_remote_ip_port_and_common() {
    let t = seeded(30);
    let (_, _, b) = send(
        &t.app,
        get("/api/v1/flows?from=now-5m&remote_ip=1.1.1.1&port=443&limit=500"),
    )
    .await;
    assert_eq!(b["meta"]["total"], 5);
    assert!(
        b["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["remote_ip"] == "1.1.1.1" && r["remote_port"] == 443)
    );
    let (_, _, none) = send(&t.app, get("/api/v1/flows?from=now-5m&direction=down")).await;
    assert_eq!(none["meta"]["total"], 0);
    let (s, _, _) = send(&t.app, get("/api/v1/flows?remote_ip=bogus")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn range_beyond_raw_retention_is_422() {
    let t = seeded(1);
    let (s, _, b) = send(&t.app, get("/api/v1/flows?from=now-30d")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["error"]["code"], "unsupported_filter_range");
    let (s, _, _) = send(&t.app, get("/api/v1/flows/export.csv?from=now-30d")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}

async fn csv(t: &TestApp, uri: &str) -> (axum::http::HeaderMap, String) {
    let res = t.app.clone().oneshot(get(uri)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let h = res.headers().clone();
    (
        h,
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap(),
    )
}

#[tokio::test]
async fn export_streams_header_and_rows() {
    let t = seeded(7);
    let (h, body) = csv(&t, "/api/v1/flows/export.csv?from=now-5m").await;
    assert!(h["content-type"].to_str().unwrap().starts_with("text/csv"));
    assert!(
        h["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment; filename=\"tmon-flows-")
    );
    let lines: Vec<&str> = body.lines().collect();
    assert!(lines[0].starts_with("ts,ts_start,exporter_id"));
    assert_eq!(lines.len(), 8);
    assert!(!body.contains("# truncated"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_truncates_at_cap() {
    let t = seeded(100_001);
    let (_, body) = csv(&t, "/api/v1/flows/export.csv?from=now-5m").await;
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 1 + 100_000 + 1);
    assert_eq!(*lines.last().unwrap(), "# truncated at 100000 rows");
}

#[tokio::test]
async fn name_source_is_exposed_in_rows_csv_and_host_detail() {
    use tmon_core::NameSource;
    let t = app();
    let now = t.state.now_ms();
    let named = rec(now - 1_000, "142.250.1.1", 443)
        .with_remote_name("rr1.googlevideo.com".into(), NameSource::Quic);
    let plain = rec(now - 2_000, "8.8.8.8", 53);
    t.state.store.append_batch(&[named, plain]).unwrap();

    let (_, _, b) = send(&t.app, get("/api/v1/flows?from=now-5m")).await;
    let rows = b["data"].as_array().unwrap();
    assert_eq!(rows[0]["remote_host_source"], "quic");
    assert!(rows[1]["remote_host_source"].is_null());

    let (_, body) = csv(&t, "/api/v1/flows/export.csv?from=now-5m").await;
    let lines: Vec<&str> = body.lines().collect();
    assert!(
        lines[0].ends_with(",remote_host,remote_host_source,app,app_confident"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].ends_with(",rr1.googlevideo.com,quic,,"),
        "{}",
        lines[1]
    );
    assert!(lines[2].ends_with(",,"), "{}", lines[2]);

    // Same rows as the host drill-down's `top_remotes`.
    let (s, _, h) = send(&t.app, get("/api/v1/top/remotes?from=now-5m")).await;
    assert_eq!(s, StatusCode::OK, "{h}");
    let remotes = h["data"].as_array().unwrap();
    let yt = remotes.iter().find(|r| r["ip"] == "142.250.1.1").unwrap();
    assert_eq!(yt["hostname_source"], "quic");
    let dns = remotes.iter().find(|r| r["ip"] == "8.8.8.8").unwrap();
    assert!(dns["hostname_source"].is_null());
}

#[tokio::test]
async fn search_and_export_sort_by_bytes_either_way() {
    let t = app();
    let now = t.state.now_ms();
    let flows: Vec<FlowRecord> = [300u64, 100, 500, 200]
        .iter()
        .enumerate()
        .map(|(i, b)| rec_sized(now - 60_000 + i as i64 * 1000, "1.1.1.1", 443, *b))
        .collect();
    t.state.store.append_batch(&flows).unwrap();
    let bytes = |b: &serde_json::Value| -> Vec<u64> {
        b["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["bytes"].as_u64().unwrap())
            .collect()
    };
    let (s, _, b) = send(&t.app, get("/api/v1/flows?from=now-5m&sort=bytes")).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(bytes(&b), [500, 300, 200, 100]);
    let (_, _, b) = send(
        &t.app,
        get("/api/v1/flows?from=now-5m&sort=bytes&order=asc"),
    )
    .await;
    assert_eq!(bytes(&b), [100, 200, 300, 500]);
    for bad in ["sort=evil", "order=sideways"] {
        let (s, _, _) = send(&t.app, get(&format!("/api/v1/flows?from=now-5m&{bad}"))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (_, body) = csv(
        &t,
        "/api/v1/flows/export.csv?from=now-5m&sort=bytes&order=asc",
    )
    .await;
    let col = tmon_store::duck::query::flows::CSV_HEADER
        .iter()
        .position(|h| *h == "bytes")
        .unwrap();
    let got: Vec<&str> = body
        .lines()
        .skip(1)
        .map(|l| l.split(',').nth(col).unwrap())
        .collect();
    assert_eq!(got, ["100", "200", "300", "500"]);
}
