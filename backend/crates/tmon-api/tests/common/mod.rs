#![allow(dead_code)]
use arc_swap::ArcSwap;
use argon2::password_hash::{PasswordHasher, SaltString};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;
use tmon_api::auth::Auth;
use tmon_api::{AppState, router};
use tmon_collector::{ExporterRegistry, GlobalCounters};
use tmon_core::LocalNetworks;
use tmon_enrich::{Pipeline, PipelineDeps};
use tmon_store::{DuckStore, StoreOptions};
use tower::ServiceExt;

pub const PASSWORD: &str = "changeme";
pub const TOKEN: &str = "test-token";

pub struct TestApp {
    pub dir: tempfile::TempDir,
    pub state: AppState,
    pub app: Router,
}

pub fn hash(pw: &str) -> String {
    let salt = SaltString::from_b64("c2FsdHNhbHRzYWx0c2FsdA").unwrap();
    argon2::Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

pub fn default_settings() -> tmon_store::duck::repo::settings::Settings {
    use tmon_store::duck::repo::settings::*;
    Settings {
        retention: Retention {
            raw_days: 7,
            rollup_1m_days: 30,
            rollup_1h_days: 365,
        },
        storage: Storage { disk_cap_gb: 20.0 },
        router: Router {
            kind: Default::default(),
            enabled: false,
            url: "https://192.168.88.1".into(),
            username: "tmon-ro".into(),
            password: "router-secret".into(),
            verify_tls: true,
            lease_poll_secs: 60,
            dns_poll_secs: 30,
        },
    }
}

pub fn app_with_clock(clock: tmon_api::state::Clock) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(DuckStore::open(dir.path(), &StoreOptions::default()).unwrap());
    let registry = Arc::new(ExporterRegistry::new());
    let globals = Arc::new(GlobalCounters::default());
    let deps = PipelineDeps {
        registry: registry.clone(),
        globals: globals.clone(),
        geo: Arc::new(tmon_enrich::NoGeo),
    };
    let pipeline = Arc::new(Pipeline::new(deps, LocalNetworks::defaults()));
    tmon_enrich::ruleset_handle::init(&store, &pipeline).unwrap();
    let hosts = tmon_enrich::hosts::HostTracker::new(store.clone());
    let state = AppState {
        hosts,
        store,
        registry: registry.clone(),
        globals,
        pipeline,
        auth: Arc::new(Auth::new(hash(PASSWORD), Some(TOKEN.into()), 24)),
        listeners: Arc::new(ArcSwap::from_pointee(Vec::new())),
        windows: Arc::new(ArcSwap::from_pointee(Default::default())),
        clock,
        live: tmon_enrich::LiveTap::new(),
        stall_after_ms: tmon_collector::exporter_registry::STALL_AFTER_MS,
        external_warnings: Arc::new(ArcSwap::from_pointee(Vec::new())),
        health: tmon_collector::health::HealthMonitor::new(registry.clone()),
        policy: Arc::new(ArcSwap::from_pointee(tmon_store::duck::retention::Policy {
            raw_ms: 7 * 86_400_000,
            rollup_1m_ms: 30 * 86_400_000,
            rollup_1h_ms: 365 * 86_400_000,
            cap_bytes: 20 << 30,
        })),
        router: Arc::new(ArcSwap::from_pointee(Default::default())),
        jobs: tmon_api::jobs::Jobs::spawn(),
        settings: Arc::new(ArcSwap::from_pointee(default_settings())),
        hits_cache: Default::default(),
        capture: None,
        dpi: None,
    };
    let app = router(state.clone());
    TestApp { dir, state, app }
}

pub fn app() -> TestApp {
    app_with_clock(Arc::new(tmon_store::duck::now_ms))
}

pub async fn send(app: &Router, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, json)
}

pub fn get(uri: &str) -> Request<Body> {
    Request::get(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap()
}

pub fn get_anon(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

pub fn json_req(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub fn login_req(pw: &str) -> Request<Body> {
    Request::post("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::json!({"password": pw}).to_string()))
        .unwrap()
}
