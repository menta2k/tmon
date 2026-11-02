//! Route table.

pub mod auth;
pub mod capture;
pub mod dpi;
pub mod exporters;
pub mod flows;
pub mod hosts;
pub mod rules;
pub mod settings;
pub mod status;
pub mod top;
pub mod traffic;

use axum::Router;
use axum::routing::{get, patch, post};

use crate::state::AppState;

/// Routes requiring authentication (mounted under `/api/v1`).
pub fn protected() -> Router<AppState> {
    Router::new()
        .route("/openapi.json", get(crate::openapi::openapi))
        .route("/exporters", get(exporters::list))
        .route("/exporters/{id}", patch(exporters::patch))
        .route("/status", get(status::status))
        .route("/live", get(crate::live::stream))
        .route("/traffic/timeseries", get(traffic::timeseries))
        .route("/traffic/breakdown", get(traffic::breakdown))
        .route("/top/hosts", get(top::hosts))
        .route("/top/remotes", get(top::remotes))
        .route("/top/asns", get(top::asns))
        .route("/top/countries", get(top::countries))
        .route("/top/ports", get(top::ports))
        .route(
            "/categories",
            get(rules::list_categories).post(rules::create_category),
        )
        .route(
            "/categories/{id}",
            patch(rules::update_category).delete(rules::delete_category),
        )
        .route("/rules", get(rules::list_rules).post(rules::create_rule))
        .route(
            "/rules/{id}",
            patch(rules::update_rule).delete(rules::delete_rule),
        )
        .route("/rules/reorder", post(rules::reorder))
        .route("/rules/test", post(rules::test_rule))
        .route("/rules/reapply", post(rules::reapply))
        .route("/jobs/{id}", get(rules::job))
        .route(
            "/networks",
            get(settings::get_networks).put(settings::put_networks),
        )
        .route(
            "/settings",
            get(settings::get_settings).patch(settings::patch_settings),
        )
        .route("/settings/export", get(settings::export))
        .route("/settings/router/test", post(settings::test_router))
        .route("/settings/import", post(settings::import))
        .route("/flows", get(flows::search))
        .route("/flows/export.csv", get(flows::export))
        .route("/hosts", get(hosts::list))
        .route("/hosts/{ip}", get(hosts::detail).patch(hosts::patch))
        .route("/applications/catalog", get(dpi::catalog))
}

/// Non-API routes (e.g. `/metrics`).
pub fn extra() -> Router<AppState> {
    Router::new().route("/metrics", get(crate::metrics::metrics))
}
