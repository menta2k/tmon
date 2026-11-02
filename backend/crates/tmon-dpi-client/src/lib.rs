//! tmon side of application classification (feature 003): spawns and
//! supervises the classifier process, taps captured packets to it over a
//! bounded queue, and keeps its answers in an [`AppTable`] for enrichment.

mod convert;
pub mod health;
mod io;
mod lru_shards;
pub mod pending;
pub mod settings;
pub mod spawn;
pub mod state;
mod supervisor;
pub mod table;
pub mod tap;

pub use health::{DpiHealth, DpiStatus};
pub use pending::PendingApps;
pub use settings::DpiSettings;
pub use state::{DpiState, DpiWarning};
pub use supervisor::{Timing, now_ms};
pub use table::AppTable;
pub use tap::DpiTap;

use std::sync::Arc;
use tmon_dpi_proto::CatalogEntry;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Everything the rest of tmon sees of application classification.
pub struct DpiHandle {
    pub settings: DpiSettings,
    pub table: Arc<AppTable>,
    pub tap: Arc<DpiTap>,
    pub health: Arc<DpiHealth>,
    pub pending: Arc<PendingApps>,
}

impl DpiHandle {
    pub fn status(&self, now_ms: i64) -> DpiStatus {
        self.health.snapshot(self.tap.counters.values(), now_ms)
    }

    pub fn warnings(&self, now_ms: i64) -> Vec<DpiWarning> {
        self.status(now_ms).warnings()
    }

    /// A handle without a running classifier, plus the receiving end of its
    /// packet queue (used by [`start`] and by tests).
    pub fn new(settings: DpiSettings) -> (Self, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        let (tap, rx) = DpiTap::new(
            settings.queue_capacity,
            settings.queue_max_bytes,
            settings.max_packets_per_conn,
            settings.max_flows as usize,
        );
        let handle = Self {
            table: Arc::new(AppTable::new(
                settings.app_table_capacity(),
                settings.idle_timeout_ms as i64,
            )),
            tap: Arc::new(tap),
            health: Arc::new(DpiHealth::new(&settings.binary)),
            pending: Arc::new(PendingApps::default()),
            settings,
        };
        (handle, rx)
    }

    /// Applications known to the classifier, sorted by name.
    pub fn catalog(&self) -> Vec<CatalogEntry> {
        let mut c = (*self.health.catalog()).clone();
        c.sort_by_key(|e| e.app.to_ascii_lowercase());
        c.dedup_by(|a, b| a.app.eq_ignore_ascii_case(&b.app));
        c
    }
}

/// Build the handle and start supervising the classifier until `stop` flips.
pub fn start(
    settings: DpiSettings,
    timing: Timing,
    stop: watch::Receiver<bool>,
) -> (Arc<DpiHandle>, JoinHandle<()>) {
    let (handle, rx) = DpiHandle::new(settings);
    let handle = Arc::new(handle);
    let sup = supervisor::Supervisor {
        settings: handle.settings.clone(),
        timing,
        sinks: io::Sinks {
            table: Arc::clone(&handle.table),
            tap: Arc::clone(&handle.tap),
            pending: Arc::clone(&handle.pending),
            health: Arc::clone(&handle.health),
        },
        queue: Arc::new(tokio::sync::Mutex::new(rx)),
        stop,
    };
    let task = tokio::spawn(sup.run());
    (handle, task)
}
