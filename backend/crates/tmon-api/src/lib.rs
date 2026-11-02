//! tmon-api: axum HTTP API (contracts/http-api.md), SSE, metrics and the SPA.
#![recursion_limit = "512"]

pub mod auth;
pub mod envelope;
pub mod jobs;
pub mod live;
pub mod metrics;
mod metrics_capture;
mod metrics_dpi;
pub mod openapi;
pub mod params;
pub mod routes;
pub mod spa;
pub mod state;
pub mod warnings;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::middleware;
use axum::routing::{get, post};
use std::time::Duration;
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub use state::AppState;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'self'";

/// Build the complete router.
pub fn router(state: AppState) -> Router {
    let protected = routes::protected().route_layer(middleware::from_fn_with_state(
        state.clone(),
        auth::require_auth,
    ));
    let public = Router::new()
        .route("/auth/login", post(routes::auth::login))
        .route("/auth/logout", post(routes::auth::logout))
        .route("/health", get(routes::auth::health));
    let api = Router::new()
        .merge(public)
        .merge(protected)
        .fallback(api_not_found);
    Router::new()
        .nest("/api/v1", api)
        .merge(routes::extra())
        .fallback(spa::serve)
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn api_not_found() -> envelope::ApiError {
    envelope::ApiError::NotFound
}
