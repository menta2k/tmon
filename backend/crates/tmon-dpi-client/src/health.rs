//! Classifier health: state, counters, drop-share window and the
//! `/status.dpi` snapshot (contracts/http-api.md).

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard};
use tmon_dpi_proto::{CatalogEntry, Hello, PROTO_VERSION, Stats};

use crate::state::{DpiState, DpiWarning};
use crate::tap::TapValues;

/// Failures within this window count towards `failed` and `restarting`.
pub const RESTART_WINDOW_MS: i64 = 10 * 60_000;
/// Failures within [`RESTART_WINDOW_MS`] that make the supervisor give up.
pub const MAX_FAILURES: usize = 5;
/// Drop-share window (FR-020) and threshold.
pub const DROP_WINDOW_MS: i64 = 15 * 60_000;
pub const DROP_SHARE_WARN: f64 = 0.05;
/// Restarts in the window that raise `dpi_restarting`.
pub const RESTARTING_WARN: usize = 2;
const SAMPLE_EVERY_MS: i64 = 60_000;

/// Counters bumped by the I/O tasks.
#[derive(Debug, Default)]
pub struct IoCounters {
    pub ipc_invalid: AtomicU64,
    pub app_invalid: AtomicU64,
    pub classifications: AtomicU64,
    pub classifications_dropped: AtomicU64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineInfo {
    pub engine: String,
    pub impl_name: String,
    pub impl_version: String,
    pub proto_version: u16,
}

#[derive(Default)]
struct Inner {
    state: Option<DpiState>,
    binary: String,
    error: Option<String>,
    engine: Option<EngineInfo>,
    running_since_ms: Option<i64>,
    failures: VecDeque<i64>,
    restarts_total: u64,
    stats: Option<(Stats, i64)>,
    catalog: Arc<Vec<CatalogEntry>>,
    incompatible: Option<u16>,
    /// (time, sent, dropped_queue) samples, one per minute.
    samples: VecDeque<(i64, u64, u64)>,
}

#[derive(Default)]
pub struct DpiHealth {
    inner: Mutex<Inner>,
    pub io: IoCounters,
}

impl DpiHealth {
    pub fn new(binary: &str) -> Self {
        let h = Self::default();
        h.lock().binary = binary.to_string();
        h.lock().state = Some(DpiState::Starting);
        h
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state(&self) -> DpiState {
        self.lock().state.unwrap_or(DpiState::Disabled)
    }

    pub fn set_state(&self, s: DpiState, error: Option<String>) {
        let mut i = self.lock();
        i.state = Some(s);
        if error.is_some() {
            i.error = error;
        }
        if s != DpiState::Running {
            i.running_since_ms = None;
        }
    }

    pub fn running(&self, hello: &Hello, now_ms: i64) {
        let mut i = self.lock();
        i.state = Some(DpiState::Running);
        i.running_since_ms = Some(now_ms);
        i.engine = Some(EngineInfo {
            engine: hello.engine.clone(),
            impl_name: hello.impl_name.clone(),
            impl_version: hello.impl_version.clone(),
            proto_version: hello.proto_version,
        });
        i.catalog = Arc::new(hello.catalog.clone());
    }

    pub fn incompatible(&self, theirs: u16) {
        let mut i = self.lock();
        i.state = Some(DpiState::Incompatible);
        i.incompatible = Some(theirs);
        i.error = Some(format!(
            "classifier protocol v{theirs}, tmon needs v{PROTO_VERSION}"
        ));
    }

    /// Record a classifier failure; returns failures within the window.
    pub fn failure(&self, error: String, now_ms: i64) -> usize {
        let mut i = self.lock();
        i.error = Some(error);
        i.running_since_ms = None;
        i.restarts_total += 1;
        i.failures.push_back(now_ms);
        while i
            .failures
            .front()
            .is_some_and(|t| *t + RESTART_WINDOW_MS < now_ms)
        {
            i.failures.pop_front();
        }
        i.failures.len()
    }

    pub fn add_catalog(&self, entries: &[CatalogEntry]) {
        let mut i = self.lock();
        let room = tmon_dpi_proto::MAX_CATALOG.saturating_sub(i.catalog.len());
        if room == 0 || entries.is_empty() {
            return;
        }
        let mut next = (*i.catalog).clone();
        next.extend(entries.iter().take(room).cloned());
        i.catalog = Arc::new(next);
    }

    pub fn catalog(&self) -> Arc<Vec<CatalogEntry>> {
        Arc::clone(&self.lock().catalog)
    }

    pub fn stats(&self, s: Stats, now_ms: i64) {
        self.lock().stats = Some((s, now_ms));
    }

    /// Time of the last heartbeat (Stats).
    pub fn last_stats_ms(&self) -> Option<i64> {
        self.lock().stats.as_ref().map(|(_, t)| *t)
    }

    /// Sample the tap counters for the drop-share window (call periodically).
    pub fn sample(&self, tap: TapValues, now_ms: i64) {
        let mut i = self.lock();
        if i.samples
            .back()
            .is_some_and(|(t, _, _)| now_ms - t < SAMPLE_EVERY_MS)
        {
            return;
        }
        i.samples.push_back((now_ms, tap.sent, tap.dropped_queue));
        while i.samples.len() > 1 && i.samples[1].0 + DROP_WINDOW_MS <= now_ms {
            i.samples.pop_front();
        }
    }

    pub fn snapshot(&self, tap: TapValues, now_ms: i64) -> DpiStatus {
        let i = self.lock();
        let (sent0, drop0) = i.samples.front().map_or((0, 0), |(_, s, d)| (*s, *d));
        let sent = tap.sent.saturating_sub(sent0);
        let dropped = tap.dropped_queue.saturating_sub(drop0);
        let share = if sent + dropped == 0 {
            0.0
        } else {
            dropped as f64 / (sent + dropped) as f64
        };
        let recent = i
            .failures
            .iter()
            .filter(|t| **t + RESTART_WINDOW_MS >= now_ms)
            .count();
        let stats = i.stats.as_ref().map(|(s, _)| s.clone()).unwrap_or_default();
        DpiStatus {
            enabled: true,
            state: i.state.unwrap_or(DpiState::Disabled),
            engine: i.engine.as_ref().map(|e| e.engine.clone()),
            impl_version: i.engine.as_ref().map(|e| e.impl_version.clone()),
            proto_version: i.engine.as_ref().map(|e| e.proto_version),
            uptime_secs: i
                .running_since_ms
                .map(|t| ((now_ms - t).max(0) / 1000) as u64),
            restarts_10m: recent,
            restarts_total: i.restarts_total,
            error: i.error.clone(),
            binary: i.binary.clone(),
            incompatible_version: i.incompatible,
            queue: QueueStatus {
                sent: tap.sent,
                dropped_queue: tap.dropped_queue,
                dropped_down: tap.dropped_down,
                skipped_final: tap.skipped_final,
                drop_share_15m: share,
            },
            classifier: ClassifierStatus::from(&stats),
            ipc_invalid: self.io.ipc_invalid.load(Relaxed),
            app_invalid: self.io.app_invalid.load(Relaxed),
            top_apps: stats
                .top
                .iter()
                .map(|t| TopAppStatus {
                    app: t.app.clone(),
                    flows: u64::from(t.flows),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueueStatus {
    pub sent: u64,
    pub dropped_queue: u64,
    pub dropped_down: u64,
    pub skipped_final: u64,
    pub drop_share_15m: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClassifierStatus {
    pub packets_in: u64,
    pub packets_invalid: u64,
    pub flows_active: u64,
    pub classified_confident: u64,
    pub classified_guess: u64,
    pub evicted_lru: u64,
    pub evicted_idle: u64,
    pub evicted_memory: u64,
    pub evicted: u64,
    pub mem_bytes: u64,
    pub mem_limit_bytes: u64,
}

impl From<&Stats> for ClassifierStatus {
    fn from(s: &Stats) -> Self {
        Self {
            packets_in: s.packets_in,
            packets_invalid: s.packets_invalid,
            flows_active: u64::from(s.flows_active),
            classified_confident: s.classified_confident,
            classified_guess: s.classified_guess,
            evicted_lru: s.evicted_lru,
            evicted_idle: s.evicted_idle,
            evicted_memory: s.evicted_memory,
            evicted: s.evicted_lru + s.evicted_idle + s.evicted_memory,
            mem_bytes: s.mem_bytes,
            mem_limit_bytes: s.mem_limit_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopAppStatus {
    pub app: String,
    pub flows: u64,
}

/// `/status.dpi` when enabled.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DpiStatus {
    pub enabled: bool,
    pub state: DpiState,
    pub engine: Option<String>,
    pub impl_version: Option<String>,
    pub proto_version: Option<u16>,
    pub uptime_secs: Option<u64>,
    pub restarts_10m: usize,
    pub restarts_total: u64,
    pub error: Option<String>,
    #[serde(skip)]
    pub binary: String,
    #[serde(skip)]
    pub incompatible_version: Option<u16>,
    pub queue: QueueStatus,
    pub classifier: ClassifierStatus,
    pub ipc_invalid: u64,
    pub app_invalid: u64,
    pub top_apps: Vec<TopAppStatus>,
}

impl DpiStatus {
    /// Current warnings (FR-020).
    pub fn warnings(&self) -> Vec<DpiWarning> {
        let mut out = Vec::new();
        match self.state {
            DpiState::NotFound => out.push(DpiWarning::NotFound {
                binary: self.binary.clone(),
            }),
            DpiState::Incompatible => out.push(DpiWarning::Incompatible {
                theirs: self.incompatible_version.unwrap_or(0),
                ours: PROTO_VERSION,
            }),
            DpiState::Failed => out.push(DpiWarning::Failed {
                error: self.error.clone().unwrap_or_else(|| "unknown error".into()),
            }),
            _ if self.restarts_10m >= RESTARTING_WARN => out.push(DpiWarning::Restarting {
                count: self.restarts_10m,
            }),
            _ => {}
        }
        if self.queue.drop_share_15m > DROP_SHARE_WARN {
            out.push(DpiWarning::Dropping {
                share: self.queue.drop_share_15m,
            });
        }
        out
    }
}
