//! Shared application state handed to every handler.

use arc_swap::ArcSwap;
use std::sync::Arc;
use tmon_collector::listener::ListenerInfo;
use tmon_collector::{ExporterRegistry, GlobalCounters};
use tmon_enrich::{LiveTap, Pipeline};
use tmon_store::DuckStore;
use tmon_store::duck::query::router::Windows;

use crate::auth::Auth;
use crate::warnings::Warning;
use tmon_collector::health::HealthMonitor;
use tmon_enrich::routeros::RouterStatus;
use tmon_store::duck::retention::Policy;

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Rule hit counts (`hits_24h`) and when they were computed (epoch ms).
pub type HitsSnapshot = (i64, Arc<std::collections::HashMap<u32, u64>>);

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<DuckStore>,
    pub registry: Arc<ExporterRegistry>,
    pub globals: Arc<GlobalCounters>,
    pub pipeline: Arc<Pipeline>,
    pub auth: Arc<Auth>,
    pub listeners: Arc<ArcSwap<Vec<ListenerInfo>>>,
    pub windows: Arc<ArcSwap<Windows>>,
    pub clock: Clock,
    pub live: Arc<LiveTap>,
    pub hosts: Arc<tmon_enrich::hosts::HostTracker>,
    /// Exporter silence before a stall warning (FR-019).
    pub stall_after_ms: i64,
    /// Warnings raised by background components (router poller, janitor…), keyed by code.
    pub external_warnings: Arc<ArcSwap<Vec<Warning>>>,
    pub health: Arc<HealthMonitor>,
    pub policy: Arc<ArcSwap<Policy>>,
    pub router: Arc<ArcSwap<RouterStatus>>,
    pub jobs: Arc<crate::jobs::Jobs>,
    pub settings: Arc<ArcSwap<tmon_store::duck::repo::settings::Settings>>,
    /// `(computed_at, rule_id → hits)` for `hits_24h`.
    pub hits_cache: Arc<std::sync::Mutex<Option<HitsSnapshot>>>,
    /// Capture input (feature 002); `None` when disabled.
    pub capture: Option<Arc<tmon_capture::CaptureHandle>>,
    /// Application classification (feature 003); `None` when disabled.
    pub dpi: Option<Arc<tmon_dpi_client::DpiHandle>>,
}

impl AppState {
    pub fn now_ms(&self) -> i64 {
        (self.clock)()
    }

    pub fn windows(&self) -> Windows {
        **self.windows.load()
    }

    pub fn policy(&self) -> Policy {
        **self.policy.load()
    }

    /// Run blocking store work off the async runtime.
    pub async fn blocking<T, F>(&self, f: F) -> Result<T, crate::envelope::ApiError>
    where
        T: Send + 'static,
        F: FnOnce(&DuckStore) -> tmon_store::StoreResult<T> + Send + 'static,
    {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || f(&store))
            .await
            .map_err(|e| crate::envelope::ApiError::Internal(e.to_string()))?
            .map_err(Into::into)
    }
}
