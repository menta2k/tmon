//! `POST /auth/login`, `POST /auth/logout`, `GET /health`.

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::auth::{client_ip, session_cookie, set_cookie};
use crate::envelope::{ApiError, ApiResult, ok};
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    password: String,
}

pub async fn login(
    State(s): State<AppState>,
    info: Option<Extension<ConnectInfo<SocketAddr>>>,
    body: Result<Json<Login>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, ApiError> {
    let ip = client_ip(info.as_ref().map(|e| &e.0));
    s.auth.check_rate(ip, s.now_ms())?;
    let Json(body) = body.map_err(|e| ApiError::validation(e.body_text()))?;
    let auth = s.auth.clone();
    let password = body.password;
    let valid = tokio::task::spawn_blocking(move || auth.verify_password(&password))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    if !valid {
        return Err(ApiError::Unauthorized);
    }
    s.auth.reset_rate(ip);
    let id = s.auth.create_session(s.now_ms())?;
    let cookie = HeaderValue::from_str(&set_cookie(&id, s.auth.ttl_secs()))
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let mut res = ok(serde_json::json!({"authenticated": true})).into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie);
    Ok(res)
}

pub async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(id) = session_cookie(&headers) {
        s.auth.end_session(&id);
    }
    let mut res = ok(serde_json::json!({"authenticated": false})).into_response();
    res.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("tmon_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    res
}

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    checks: Checks,
}

#[derive(Serialize)]
pub struct Checks {
    storage: bool,
    geoip: bool,
    collectors: bool,
}

pub async fn health(State(s): State<AppState>) -> ApiResult<Health> {
    let storage = s
        .blocking(|st| {
            Ok(st
                .conn()?
                .query_row("SELECT 1", [], |r| r.get::<_, i32>(0))?
                == 1)
        })
        .await
        .unwrap_or(false);
    let geoip = s.pipeline.deps().geo.status().loaded;
    let collectors = s.listeners.load().iter().all(|l| l.listening);
    let status = if storage && geoip && collectors {
        "ok"
    } else {
        "degraded"
    };
    ok(Health {
        status,
        checks: Checks {
            storage,
            geoip,
            collectors,
        },
    })
}
