//! `GET /exporters`, `PATCH /exporters/{id}` (FR-004/FR-006).

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::Value;
use tmon_collector::ExporterInfo;
use tmon_collector::exporter_registry::{CountDirection, ExporterSettings};
use tmon_core::ValidationErrors;
use tmon_store::duck::repo::exporters::{self, ExporterRow};

use crate::envelope::{ApiError, ApiResult, ok};
use crate::state::AppState;

const NAME_MAX: usize = 64;
const SAMPLING_MAX: u32 = 1 << 24;

pub async fn list(State(s): State<AppState>) -> ApiResult<Vec<ExporterInfo>> {
    ok(s.registry.snapshot(s.now_ms(), s.stall_after_ms))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchBody {
    #[serde(default, deserialize_with = "nullable")]
    name: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    sampling_override: Option<Option<u32>>,
    count_direction: Option<String>,
}

fn nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::<T>::deserialize(d)?))
}

/// Apply a patch to current settings, validating every field.
fn apply(cur: &ExporterSettings, b: PatchBody) -> Result<ExporterSettings, ValidationErrors> {
    let mut e = ValidationErrors::new();
    let name = b
        .name
        .map(|n| n.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| cur.name.clone());
    if name.as_ref().is_some_and(|n| n.chars().count() > NAME_MAX) {
        e.add("name", format!("must be at most {NAME_MAX} characters"));
    }
    let sampling_override = b.sampling_override.unwrap_or(cur.sampling_override);
    if sampling_override.is_some_and(|n| n == 0 || n > SAMPLING_MAX) {
        e.add("sampling_override", format!("must be 1–{SAMPLING_MAX}"));
    }
    let count_direction = match b.count_direction {
        None => cur.count_direction,
        Some(d) => CountDirection::parse(&d).unwrap_or_else(|| {
            e.add("count_direction", "must be ingress|egress|both");
            cur.count_direction
        }),
    };
    e.into_result(ExporterSettings {
        name,
        sampling_override,
        count_direction,
    })
}

pub async fn patch(
    State(s): State<AppState>,
    Path(id): Path<u16>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<ExporterInfo> {
    let Json(v) = body.map_err(|e| ApiError::validation(e.body_text()))?;
    let b: PatchBody =
        serde_json::from_value(v).map_err(|e| ApiError::validation(e.to_string()))?;
    let exporter = s.registry.get_by_id(id).ok_or(ApiError::NotFound)?;
    let next = apply(&exporter.settings(), b)?;
    s.registry.update_settings(id, next);
    let info = exporter.info(s.now_ms(), s.stall_after_ms);
    let row = ExporterRow {
        id: info.id,
        address: info.address.to_string(),
        domain_id: info.domain_id,
        protocol: info.protocol.as_str().to_string(),
        name: info.name.clone(),
        sampling_override: info.sampling_override,
        count_direction: info.count_direction.as_str().to_string(),
        first_seen: info.first_seen,
        last_seen: info.last_seen,
    };
    s.blocking(move |st| st.with_writer(|c| exporters::upsert(c, &row)))
        .await?;
    ok(info)
}
