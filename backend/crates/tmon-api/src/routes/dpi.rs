//! Application classification views (feature 003): `/status.dpi`,
//! `GET /applications/catalog` and the classifier warnings.

use axum::extract::State;
use serde::Serialize;
use tmon_dpi_client::{DpiHandle, DpiStatus};

use crate::envelope::{ApiResult, ok};
use crate::state::AppState;
use crate::warnings::Warning;

/// `/status.dpi`: always present, `{"enabled": false}` when DPI is off.
#[derive(Serialize)]
#[serde(untagged)]
pub enum DpiView {
    Disabled { enabled: bool },
    Enabled(Box<DpiStatus>),
}

pub fn status(h: Option<&DpiHandle>, now_ms: i64) -> DpiView {
    match h {
        Some(h) => DpiView::Enabled(Box::new(h.status(now_ms))),
        None => DpiView::Disabled { enabled: false },
    }
}

/// Classifier warnings (FR-020) in the banner shape.
pub fn warnings(h: &DpiHandle, now_ms: i64) -> Vec<Warning> {
    h.warnings(now_ms)
        .into_iter()
        .map(|w| Warning::new(w.code(), w.message()))
        .collect()
}

#[derive(Debug, Serialize)]
pub struct CatalogApp {
    pub app: String,
    pub category: String,
}

/// Applications the running classifier can report (rule editor picker).
pub async fn catalog(State(s): State<AppState>) -> ApiResult<Vec<CatalogApp>> {
    let apps = s
        .dpi
        .as_deref()
        .map(|h| {
            h.catalog()
                .into_iter()
                .map(|e| CatalogApp {
                    app: e.app,
                    category: e.category,
                })
                .collect()
        })
        .unwrap_or_default();
    ok(apps)
}
