//! Local host tracking: records first/last seen per local IP and keeps a display-name map.

use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tmon_core::{Direction, FlowRecord};
use tmon_store::duck::repo::hosts;
use tmon_store::{DuckStore, StoreResult};

use crate::pipeline::BatchHook;

pub const FLUSH_EVERY: Duration = Duration::from_secs(10);

/// ip → (first_ts, last_ts) seen since the last flush.
type Pending = HashMap<String, (i64, i64)>;

pub struct HostTracker {
    store: Arc<DuckStore>,
    pending: Mutex<Pending>,
    names: ArcSwap<HashMap<String, String>>,
}

impl HostTracker {
    pub fn new(store: Arc<DuckStore>) -> Arc<Self> {
        Arc::new(Self {
            store,
            pending: Mutex::new(Pending::new()),
            names: ArcSwap::from_pointee(HashMap::new()),
        })
    }

    pub fn record(&self, flows: &[FlowRecord]) {
        let Ok(mut p) = self.pending.lock() else {
            tracing::error!("host tracker poisoned");
            return;
        };
        for f in flows.iter().filter(|f| f.direction != Direction::Transit) {
            let e = p
                .entry(f.local_ip.to_string())
                .or_insert((f.ts_ms, f.ts_ms));
            *e = (e.0.min(f.ts_ms), e.1.max(f.ts_ms));
        }
    }

    pub fn hook(self: &Arc<Self>) -> BatchHook {
        let me = Arc::clone(self);
        Arc::new(move |flows| me.record(flows))
    }

    /// Persist pending sightings (blocking).
    pub fn flush(&self) -> StoreResult<usize> {
        let batch: Vec<(String, i64, i64)> = match self.pending.lock() {
            Ok(mut p) => std::mem::take(&mut *p)
                .into_iter()
                .map(|(ip, (a, b))| (ip, a, b))
                .collect(),
            Err(_) => return Err(tmon_store::StoreError::Task("host tracker poisoned".into())),
        };
        if !batch.is_empty() {
            self.store.with_writer(|c| hosts::touch(c, &batch))?;
        }
        Ok(batch.len())
    }

    /// Reload display names (after edits or DHCP updates).
    pub fn reload_names(&self) -> StoreResult<()> {
        self.names
            .store(Arc::new(hosts::names(&self.store.conn()?)?));
        Ok(())
    }

    pub fn display_name(&self, ip: &str) -> String {
        self.names
            .load()
            .get(ip)
            .cloned()
            .unwrap_or_else(|| ip.to_string())
    }

    pub fn spawn(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let mut t = tokio::time::interval(FLUSH_EVERY);
            loop {
                t.tick().await;
                let m = Arc::clone(&me);
                match tokio::task::spawn_blocking(move || {
                    m.flush()
                        .and_then(|n| if n > 0 { m.reload_names() } else { Ok(()) })
                })
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => tracing::error!(error = %e, "persisting local hosts failed"),
                    Err(e) => tracing::error!(error = %e, "host tracker task failed"),
                }
            }
        })
    }
}
