mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;

#[tokio::test]
async fn login_sets_strict_httponly_cookie_and_grants_access() {
    let t = app();
    let (status, headers, body) = send(&t.app, login_req(PASSWORD)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    let cookie = headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(cookie.starts_with("tmon_session="));
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    let session = cookie.split(';').next().unwrap().to_string();
    let req = Request::get("/api/v1/exporters")
        .header(header::COOKIE, session.clone())
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(&t.app, req).await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
    let logout = Request::post("/api/v1/auth/logout")
        .header(header::COOKIE, session.clone())
        .body(Body::empty())
        .unwrap();
    send(&t.app, logout).await;
    let req = Request::get("/api/v1/exporters")
        .header(header::COOKIE, session)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&t.app, req).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_password_is_unauthorized_with_envelope() {
    let t = app();
    let (status, _, body) = send(&t.app, login_req("nope")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["ok"], false);
    assert_eq!(body["data"], serde_json::Value::Null);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert!(body["error"]["message"].is_string());
}

#[tokio::test]
async fn sixth_login_attempt_within_a_minute_is_rate_limited() {
    let t = app();
    for _ in 0..5 {
        assert_eq!(
            send(&t.app, login_req("nope")).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, _, body) = send(&t.app, login_req(PASSWORD)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "rate_limited");
}

#[tokio::test]
async fn protected_route_requires_auth_and_accepts_bearer() {
    let t = app();
    assert_eq!(
        send(&t.app, get_anon("/api/v1/exporters")).await.0,
        StatusCode::UNAUTHORIZED
    );
    let bad = Request::get("/api/v1/exporters")
        .header(header::AUTHORIZATION, "Bearer wrong")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&t.app, bad).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        send(&t.app, get("/api/v1/exporters")).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn health_needs_no_auth() {
    let t = app();
    let (status, _, body) = send(&t.app, get_anon("/api/v1/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["checks"]["storage"], true);
    assert!(body["data"]["status"] == "ok" || body["data"]["status"] == "degraded");
}

#[tokio::test]
async fn unknown_query_parameter_is_rejected() {
    let t = app();
    let (status, _, body) =
        send(&t.app, get("/api/v1/traffic/breakdown?by=category&bogus=1")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "validation_failed");
    assert_eq!(body["error"]["fields"]["bogus"], "unknown query parameter");
}

#[tokio::test]
async fn unknown_api_route_is_404_envelope_and_spa_fallback_serves_html() {
    let t = app();
    let (status, _, body) = send(&t.app, get("/api/v1/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    let res = send(&t.app, get_anon("/some/client/route")).await;
    assert_eq!(res.0, StatusCode::OK);
    assert!(res.1.get(header::CONTENT_SECURITY_POLICY).is_some());
}
