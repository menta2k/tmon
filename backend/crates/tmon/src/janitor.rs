//! Runs the retention janitor at startup and hourly; raises a disk-cap warning.

use arc_swap::ArcSwap;
use std::sync::Arc;
use std::time::Duration;
use tmon_collector::health::Warning;
use tmon_store::DuckStore;
use tmon_store::duck::retention::{self, Policy};

pub const JANITOR_EVERY: Duration = Duration::from_secs(3600);
const CAP_CODE: &str = "disk_cap";

/// Replace the disk-cap warning in the shared list (immutable update).
pub fn with_cap_warning(current: &[Warning], warning: Option<Warning>) -> Vec<Warning> {
    current
        .iter()
        .filter(|w| w.code != CAP_CODE)
        .cloned()
        .chain(warning)
        .collect()
}

pub fn spawn(
    store: Arc<DuckStore>,
    policy: Arc<ArcSwap<Policy>>,
    warnings: Arc<ArcSwap<Vec<Warning>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut t = tokio::time::interval(JANITOR_EVERY);
        loop {
            t.tick().await;
            let (s, p) = (Arc::clone(&store), **policy.load());
            match tokio::task::spawn_blocking(move || {
                retention::run(&s, &p, tmon_store::duck::now_ms())
            })
            .await
            {
                Ok(Ok(r)) => {
                    tracing::info!(
                        expired = r.expired_files,
                        capped = r.capped_files,
                        disk = r.disk_bytes,
                        "retention pass"
                    );
                    let w = r.cap_exceeded().then(|| {
                        Warning::new(
                            CAP_CODE,
                            format!(
                                "Disk cap of {:.1} GB reached: oldest raw flows were deleted early",
                                r.cap_bytes as f64 / 1e9
                            ),
                        )
                    });
                    warnings.store(Arc::new(with_cap_warning(&warnings.load(), w)));
                }
                Ok(Err(e)) => tracing::error!(error = %e, "retention pass failed"),
                Err(e) => tracing::error!(error = %e, "retention task failed"),
            }
        }
    })
}
