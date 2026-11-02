//! Capture input wiring (feature 002): listener, pipeline namer and the
//! DNS-name feed into `remote_hostname` + back-fill (FR-012).

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tmon_capture::{CaptureHandle, CaptureSettings};
use tmon_enrich::{Pipeline, router_poll};
use tmon_store::DuckStore;
use tmon_store::duck::repo::remote_hostnames::{self, SOURCE_CAPTURE};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// How often captured DNS names are written to `remote_hostname`.
pub const NAME_FEED_INTERVAL: Duration = Duration::from_secs(5);

/// Capture allow-list: its own, else the collector's (contracts/capture-input.md).
pub fn effective_settings(s: &CaptureSettings, allowed_exporters: &[IpAddr]) -> CaptureSettings {
    let allowed_sources = if s.allowed_sources.is_empty() {
        allowed_exporters.to_vec()
    } else {
        s.allowed_sources.clone()
    };
    CaptureSettings {
        allowed_sources,
        ..s.clone()
    }
}

/// Start capture and install it as the pipeline's naming step. `tap`
/// forwards packets to the application classifier (feature 003).
pub fn start(
    settings: CaptureSettings,
    pipeline: &Pipeline,
    stop: watch::Receiver<bool>,
    tap: Option<tmon_capture::TapConfig>,
) -> Arc<CaptureHandle> {
    let handle = tmon_capture::listener::start_with_tap(settings, stop, tap);
    if !pipeline.set_namer(handle.table.clone()) {
        tracing::warn!("a connection namer was already installed; capture names unused");
    }
    handle
}

/// Drain captured DNS names into the store every [`NAME_FEED_INTERVAL`].
pub fn spawn_name_feed(
    handle: Arc<CaptureHandle>,
    store: Arc<DuckStore>,
    pipeline: Arc<Pipeline>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(NAME_FEED_INTERVAL);
        loop {
            tick.tick().await;
            let batch = handle.names.drain();
            if batch.is_empty() {
                continue;
            }
            let (s, p, now) = (Arc::clone(&store), Arc::clone(&pipeline), clock());
            let r = tokio::task::spawn_blocking(move || feed(&s, &p, &batch, now)).await;
            match r {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "capture name feed failed"),
                Err(e) => tracing::warn!(error = %e, "capture name feed task failed"),
            }
        }
    })
}

/// Upsert names, refresh the pipeline's map and back-fill recent flows.
pub fn feed(
    store: &DuckStore,
    pipeline: &Pipeline,
    batch: &[(IpAddr, Arc<str>)],
    now_ms: i64,
) -> tmon_store::StoreResult<()> {
    let rows: Vec<(String, String)> = batch
        .iter()
        .map(|(ip, h)| (ip.to_string(), h.to_string()))
        .collect();
    store.with_writer(|c| remote_hostnames::upsert_from(c, &rows, SOURCE_CAPTURE, now_ms))?;
    pipeline.set_remote_names(router_poll::load_remote_names(store)?);
    let rules = pipeline.rules();
    let filled = tmon_store::duck::backfill::backfill_hostnames(store, now_ms, rules.as_ref())?;
    if filled.named > 0 {
        tracing::debug!(named = filled.named, "back-filled names from captured DNS");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(v: &[&str]) -> Vec<IpAddr> {
        v.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn own_allow_list_wins() {
        let s = CaptureSettings {
            allowed_sources: ips(&["10.0.0.1"]),
            ..CaptureSettings::default()
        };
        assert_eq!(
            effective_settings(&s, &ips(&["192.168.88.1"])).allowed_sources,
            ips(&["10.0.0.1"])
        );
    }

    #[test]
    fn empty_allow_list_falls_back_to_the_collector() {
        let s = CaptureSettings::default();
        assert_eq!(
            effective_settings(&s, &ips(&["192.168.88.1"])).allowed_sources,
            ips(&["192.168.88.1"])
        );
    }

    #[test]
    fn both_empty_accepts_everyone() {
        let s = effective_settings(&CaptureSettings::default(), &[]);
        assert!(s.allowed_sources.is_empty());
        assert!(tmon_capture::CaptureHandle::new(s, 0).accepts("203.0.113.9".parse().unwrap()));
    }
}
