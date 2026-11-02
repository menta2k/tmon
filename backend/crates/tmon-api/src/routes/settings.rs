//! Networks, settings and export/import (FR-007, FR-020..FR-022, FR-026).

use axum::Json;
use axum::extract::State;
use serde_json::Value;
use std::sync::Arc;
use tmon_store::duck::query::router::Windows;
use tmon_store::duck::repo::bundle;
use tmon_store::duck::repo::networks::{self, NetworkRow};
use tmon_store::duck::repo::settings::{self, Settings};
use tmon_store::duck::retention::Policy;

use crate::envelope::{ApiError, ApiResult, ok};
use crate::state::AppState;

const DAY_MS: i64 = 86_400_000;
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

type Body = Result<Json<Value>, axum::extract::rejection::JsonRejection>;

fn json_body(b: Body) -> Result<Value, ApiError> {
    b.map(|Json(v)| v)
        .map_err(|e| ApiError::validation(e.body_text()))
}

pub async fn get_networks(State(s): State<AppState>) -> ApiResult<Vec<NetworkRow>> {
    ok(s.blocking(|st| networks::list(&st.conn()?)).await?)
}

pub async fn put_networks(State(s): State<AppState>, b: Body) -> ApiResult<Vec<NetworkRow>> {
    let rows: Vec<NetworkRow> =
        serde_json::from_value(json_body(b)?).map_err(|e| ApiError::validation(e.to_string()))?;
    let pipeline = s.pipeline.clone();
    let saved = s
        .blocking(move |st| {
            let nets = st.with_writer(|c| networks::replace(c, &rows))?;
            pipeline.set_networks(nets);
            networks::list(&st.conn()?)
        })
        .await?;
    ok(saved)
}

/// Derived retention policy / query windows from settings.
pub fn policy_of(st: &Settings) -> Policy {
    Policy {
        raw_ms: i64::from(st.retention.raw_days) * DAY_MS,
        rollup_1m_ms: i64::from(st.retention.rollup_1m_days) * DAY_MS,
        rollup_1h_ms: i64::from(st.retention.rollup_1h_days) * DAY_MS,
        cap_bytes: (st.storage.disk_cap_gb * GIB) as u64,
    }
}

/// Publish new settings to every consumer.
pub fn publish(s: &AppState, next: Settings) {
    let p = policy_of(&next);
    s.policy.store(Arc::new(p));
    s.windows.store(Arc::new(Windows {
        raw_ms: p.raw_ms,
        rollup_1m_ms: p.rollup_1m_ms,
    }));
    s.settings.store(Arc::new(next));
}

pub async fn get_settings(State(s): State<AppState>) -> ApiResult<Value> {
    ok(s.settings.load().view())
}

pub async fn patch_settings(State(s): State<AppState>, b: Body) -> ApiResult<Value> {
    let patch = json_body(b)?;
    let next = s.settings.load().patched(&patch)?;
    let saved = next.clone();
    s.blocking(move |st| st.with_writer(|c| settings::save(c, &saved)))
        .await?;
    let view = next.view();
    publish(&s, next);
    ok(view)
}

pub async fn export(State(s): State<AppState>) -> ApiResult<Value> {
    let current = (**s.settings.load()).clone();
    ok(s.blocking(move |st| bundle::export(st, &current)).await?)
}

pub async fn import(State(s): State<AppState>, b: Body) -> ApiResult<Value> {
    let v = json_body(b)?;
    // Accept the raw bundle or an envelope `{ok, data: bundle}` as produced by export.
    let v = if v.get("ok").is_some() && v.get("data").is_some() {
        v["data"].clone()
    } else {
        v
    };
    let current = (**s.settings.load()).clone();
    let b = bundle::validate(&v, &current)?;
    let (now, pipeline, hosts) = (s.now_ms(), s.pipeline.clone(), s.hosts.clone());
    let applied = b.clone();
    s.blocking(move |st| {
        bundle::apply(st, &applied, now)?;
        pipeline.set_networks(networks::load(&st.conn()?)?);
        tmon_enrich::ruleset_handle::reload(st, &pipeline)?;
        hosts.reload_names()
    })
    .await?;
    publish(&s, b.settings);
    *s.hits_cache
        .lock()
        .map_err(|_| ApiError::Internal("hits cache poisoned".into()))? = None;
    ok(
        serde_json::json!({"imported": {"categories": b.categories.len(), "rules": b.rules.len(), "hosts": b.hosts.len(), "networks": b.networks.len()}}),
    )
}

/// `POST /settings/router/test`: body is a partial router section; the stored password is used when omitted.
pub async fn test_router(State(s): State<AppState>, b: Body) -> ApiResult<Value> {
    let patch = json_body(b)?;
    let current = s.settings.load();
    let candidate = current.patched(&serde_json::json!({ "router": patch }))?;
    // Also when certificate checks are turned off: the stored password must not
    // go out over unverified TLS without being re-entered.
    let target_changed = candidate.router.url != current.router.url
        || candidate.router.username != current.router.username
        || (current.router.verify_tls && !candidate.router.verify_tls);
    if target_changed
        && patch
            .get("password")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        // Never send the stored credential to a URL/user it was not saved for.
        return Err(ApiError::field(
            "password",
            "required when testing a different url or username, or without TLS verification",
        ));
    }
    let result = tmon_enrich::router_poll::test_connection(&candidate.router).await;
    ok(match result {
        Ok(n) => serde_json::json!({"reachable": true, "leases": n}),
        Err(e) => serde_json::json!({"reachable": false, "error": e.to_string()}),
    })
}
