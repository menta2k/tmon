//! Application classification wiring (feature 003): classifier supervisor,
//! capture tap, pipeline step and the late-classification back-fill.

use std::sync::Arc;
use std::time::Duration;
use tmon_capture::{CaptureHandle, TapConfig};
use tmon_dpi_client::{DpiHandle, Timing};
use tmon_enrich::Pipeline;
use tmon_store::DuckStore;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::Validated;

/// How often late classifications are back-filled into stored flows.
pub const BACKFILL_INTERVAL: Duration = Duration::from_secs(5);

/// Running capture and classification inputs.
#[derive(Default)]
pub struct Inputs {
    pub capture: Option<Arc<CaptureHandle>>,
    pub dpi: Option<Arc<DpiHandle>>,
    pub tasks: Vec<JoinHandle<()>>,
}

/// Start capture (feature 002) and, when enabled, the classifier with a tap
/// on the capture processor. With `[dpi]` absent nothing extra is created
/// and no process is spawned (SC-007).
pub fn start_inputs(
    v: &Validated,
    pipeline: &Arc<Pipeline>,
    store: &Arc<DuckStore>,
    clock: &tmon_api::state::Clock,
    stop: &watch::Receiver<bool>,
) -> Inputs {
    let Some(capture) = &v.capture else {
        return Inputs::default();
    };
    let mut tasks = Vec::new();
    let dpi = v.dpi.clone().map(|settings| {
        let (handle, task) = tmon_dpi_client::start(settings, Timing::default(), stop.clone());
        tasks.push(task);
        assert!(
            pipeline.set_classifier(handle.table.clone()),
            "classifier installed once"
        );
        handle
    });
    let tap = dpi.as_ref().map(|h| tap_config(h, pipeline));
    let settings = crate::capture::effective_settings(capture, &v.allowed_exporters);
    let handle = crate::capture::start(settings, pipeline, stop.clone(), tap);
    tasks.push(crate::capture::spawn_name_feed(
        Arc::clone(&handle),
        Arc::clone(store),
        Arc::clone(pipeline),
        Arc::clone(clock),
    ));
    if let Some(h) = &dpi {
        tasks.push(spawn_backfill(
            Arc::clone(h),
            Arc::clone(store),
            Arc::clone(pipeline),
            Arc::clone(clock),
        ));
    }
    Inputs {
        capture: Some(handle),
        dpi,
        tasks,
    }
}

/// The tap orients packets by the pipeline's current local networks.
fn tap_config(h: &DpiHandle, pipeline: &Arc<Pipeline>) -> TapConfig {
    let p = Arc::clone(pipeline);
    TapConfig {
        tap: h.tap.clone(),
        is_local: Arc::new(move |ip| p.networks().contains(ip)),
    }
}

/// Apply late classifications to recent flows every [`BACKFILL_INTERVAL`].
pub fn spawn_backfill(
    handle: Arc<DpiHandle>,
    store: Arc<DuckStore>,
    pipeline: Arc<Pipeline>,
    clock: tmon_api::state::Clock,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(BACKFILL_INTERVAL);
        loop {
            tick.tick().await;
            let items = handle.pending.drain();
            if items.is_empty() {
                continue;
            }
            let (s, rules, now) = (Arc::clone(&store), pipeline.rules(), clock());
            let r = tokio::task::spawn_blocking(move || {
                tmon_store::duck::backfill_apps::backfill_apps(&s, now, rules.as_ref(), &items)
            })
            .await;
            match r {
                Ok(Ok(b)) if b.named > 0 => {
                    tracing::debug!(named = b.named, "back-filled applications")
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "application back-fill failed"),
                Err(e) => tracing::warn!(error = %e, "application back-fill task failed"),
            }
        }
    })
}
