mod common;

use axum::http::StatusCode;
use common::*;
use http_body_util::BodyExt;
use std::time::Duration;
use tower::ServiceExt;

async fn next_chunk(body: &mut axum::body::Body) -> String {
    let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
        .await
        .expect("sse frame")
        .unwrap()
        .unwrap();
    String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap()
}

#[tokio::test]
async fn live_streams_ticks_and_warnings() {
    let t = app();
    let res = t.app.clone().oneshot(get("/api/v1/live")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/event-stream");
    let mut body = res.into_body();

    // Initial warning snapshot (GeoIP is not loaded in tests).
    let first = next_chunk(&mut body).await;
    assert!(first.starts_with("event: warning"), "{first}");
    assert!(first.contains("geoip_missing"));

    t.state.live.flush(1_000, tmon_enrich::live_tap::TICK);
    let tick = next_chunk(&mut body).await;
    assert!(tick.starts_with("event: tick"), "{tick}");
    let data: serde_json::Value =
        serde_json::from_str(tick.lines().find_map(|l| l.strip_prefix("data: ")).unwrap()).unwrap();
    assert_eq!(data["ts"], 1_000);
    assert!(data["bytes_per_sec"]["down"].is_number());
    assert!(data["bytes_per_sec"]["up"].is_number());
    assert!(data["by_category"].is_array());
}

#[tokio::test]
async fn live_requires_auth() {
    let t = app();
    assert_eq!(
        send(&t.app, get_anon("/api/v1/live")).await.0,
        StatusCode::UNAUTHORIZED
    );
}
