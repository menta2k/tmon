//! `GET /hosts`, `GET /hosts/{ip}`, `PATCH /hosts/{ip}` (FR-012, FR-015).

use axum::Json;
use axum::extract::{Path, RawQuery, State};
use serde::Deserialize;
use serde_json::Value;
use tmon_store::duck::query::host::{HostDetail, host_detail};
use tmon_store::duck::query::top::Ctx;
use tmon_store::duck::repo::hosts::{self, HostPatch, LocalHost};

use crate::envelope::{ApiError, ApiResult, Meta, ok, ok_meta};
use crate::params::Params;
use crate::state::AppState;

pub const DEFAULT_PAGE_SIZE: u64 = 50;
pub const MAX_PAGE_SIZE: u64 = 500;

fn parse_ip(ip: &str) -> Result<String, ApiError> {
    ip.parse::<std::net::IpAddr>()
        .map(|a| a.to_string())
        .map_err(|_| ApiError::field("ip", "must be an IP address"))
}

pub async fn detail(
    State(s): State<AppState>,
    Path(ip): Path<String>,
    RawQuery(q): RawQuery,
) -> ApiResult<HostDetail> {
    let ip = parse_ip(&ip)?;
    let p = Params::parse(q.as_deref());
    p.allow(&["from", "to"], false)?;
    let f = p.filter(s.now_ms())?;
    let (w, now) = (s.windows(), s.now_ms());
    let (d, src) = s
        .blocking(move |st| {
            host_detail(
                &Ctx {
                    conn: &st.conn()?,
                    raw_dir: st.raw_dir(),
                    windows: w,
                    now_ms: now,
                },
                &ip,
                &f,
            )
        })
        .await?;
    ok_meta(
        d,
        Meta {
            source: Some(src),
            ..Meta::default()
        },
    )
}

pub async fn list(State(s): State<AppState>, RawQuery(q): RawQuery) -> ApiResult<Vec<LocalHost>> {
    let p = Params::parse(q.as_deref());
    p.allow(&["page", "limit", "q"], false)?;
    let page = p.u64_or("page", 1, 1, u64::from(u32::MAX))?;
    let limit = p.u64_or("limit", DEFAULT_PAGE_SIZE, 1, MAX_PAGE_SIZE)?;
    let search = p.one("q").map(|v| v.chars().take(100).collect::<String>());
    let (rows, total) = s
        .blocking(move |st| hosts::list(&st.conn()?, search.as_deref(), page, limit))
        .await?;
    ok_meta(
        rows,
        Meta {
            page: Some(page),
            limit: Some(limit),
            total: Some(total),
            ..Meta::default()
        },
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchBody {
    #[serde(default, deserialize_with = "nullable")]
    friendly_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    mac: Option<Option<String>>,
}

/// Distinguish "absent" (None) from explicit null (Some(None)).
fn nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::<String>::deserialize(d)?))
}

pub async fn patch(
    State(s): State<AppState>,
    Path(ip): Path<String>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<LocalHost> {
    let ip = parse_ip(&ip)?;
    let Json(v) = body.map_err(|e| ApiError::validation(e.body_text()))?;
    let b: PatchBody =
        serde_json::from_value(v).map_err(|e| ApiError::validation(e.to_string()))?;
    let patch = HostPatch {
        friendly_name: b.friendly_name,
        mac: b.mac,
    };
    let mac_changed = patch.mac.is_some();
    let pipeline = s.pipeline.clone();
    let tracker = s.hosts.clone();
    let host = s
        .blocking(move |st| {
            let h = st.with_writer(|c| hosts::patch(c, &ip, &patch))?;
            tracker.reload_names()?;
            if mac_changed {
                tmon_enrich::ruleset_handle::reload(st, &pipeline)?;
            }
            Ok(h)
        })
        .await?;
    ok(host)
}
