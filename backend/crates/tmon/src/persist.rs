//! Background persistence of exporter identity, settings and last-seen time.

use std::sync::Arc;
use std::time::Duration;
use tmon_collector::{CountDirection, FlowProtocol};
use tmon_collector::{Exporter, ExporterRegistry, ExporterSettings, KnownExporter};
use tmon_store::DuckStore;
use tmon_store::duck::repo::exporters::{self, ExporterRow};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const PERSIST_EVERY: Duration = Duration::from_secs(60);

pub fn row(e: &Exporter) -> ExporterRow {
    let s = e.settings();
    ExporterRow {
        id: e.id,
        address: e.address.to_string(),
        domain_id: e.domain_id,
        protocol: e.protocol().as_str().to_string(),
        name: s.name.clone(),
        sampling_override: s.sampling_override,
        count_direction: s.count_direction.as_str().to_string(),
        first_seen: e.first_seen,
        last_seen: e.last_seen(),
    }
}

/// Convert persisted rows into registry seeds (invalid rows are skipped).
pub fn known(rows: Vec<ExporterRow>) -> Vec<KnownExporter> {
    rows.into_iter()
        .filter_map(|r| {
            Some(KnownExporter {
                id: r.id,
                address: r.address.parse().ok()?,
                domain_id: r.domain_id,
                protocol: FlowProtocol::parse(&r.protocol)?,
                settings: ExporterSettings {
                    name: r.name,
                    sampling_override: r.sampling_override,
                    count_direction: CountDirection::parse(&r.count_direction).unwrap_or_default(),
                },
                first_seen: r.first_seen,
                last_seen: r.last_seen,
            })
        })
        .collect()
}

pub fn save(store: &DuckStore, e: &Exporter) {
    if let Err(err) = store.conn().and_then(|c| exporters::upsert(&c, &row(e))) {
        tracing::error!(error = %err, exporter = %e.address, "failed to persist exporter");
    }
}

/// Persist new exporters immediately and everyone's `last_seen` every minute.
pub fn spawn(
    store: Arc<DuckStore>,
    registry: Arc<ExporterRegistry>,
    mut rx: mpsc::UnboundedReceiver<Arc<Exporter>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(PERSIST_EVERY);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(e) => {
                        let s = Arc::clone(&store);
                        let _ = tokio::task::spawn_blocking(move || save(&s, &e)).await;
                    }
                    None => break,
                },
                _ = tick.tick() => {
                    let (s, r) = (Arc::clone(&store), Arc::clone(&registry));
                    let _ = tokio::task::spawn_blocking(move || r.all().iter().for_each(|e| save(&s, e))).await;
                }
            }
        }
    })
}
