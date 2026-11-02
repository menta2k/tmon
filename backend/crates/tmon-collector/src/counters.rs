//! Lock-free ingest counters (FR-006, US5) with immutable snapshots.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

/// Reasons a datagram or flow can be dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    NotAllowed,
    Backpressure,
    UnknownVersion,
    DirectionFilter,
    ExporterLimit,
}

impl DropReason {
    pub const ALL: [DropReason; 5] = [
        DropReason::NotAllowed,
        DropReason::Backpressure,
        DropReason::UnknownVersion,
        DropReason::DirectionFilter,
        DropReason::ExporterLimit,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            DropReason::NotAllowed => "not_allowed",
            DropReason::Backpressure => "backpressure",
            DropReason::UnknownVersion => "unknown_version",
            DropReason::DirectionFilter => "direction_filter",
            DropReason::ExporterLimit => "exporter_limit",
        }
    }
}

/// Global counters (not tied to a known exporter).
#[derive(Debug, Default)]
pub struct GlobalCounters {
    not_allowed: AtomicU64,
    backpressure: AtomicU64,
    unknown_version: AtomicU64,
    direction_filter: AtomicU64,
    exporter_limit: AtomicU64,
}

impl GlobalCounters {
    fn slot(&self, r: DropReason) -> &AtomicU64 {
        match r {
            DropReason::NotAllowed => &self.not_allowed,
            DropReason::Backpressure => &self.backpressure,
            DropReason::UnknownVersion => &self.unknown_version,
            DropReason::DirectionFilter => &self.direction_filter,
            DropReason::ExporterLimit => &self.exporter_limit,
        }
    }

    pub fn drop_n(&self, reason: DropReason, n: u64) {
        self.slot(reason).fetch_add(n, Ordering::Relaxed);
    }

    pub fn dropped(&self, reason: DropReason) -> u64 {
        self.slot(reason).load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> Vec<(DropReason, u64)> {
        DropReason::ALL
            .iter()
            .map(|r| (*r, self.dropped(*r)))
            .collect()
    }
}

/// Per-exporter counters.
#[derive(Debug, Default)]
pub struct ExporterCounters {
    received: AtomicU64,
    decoded: AtomicU64,
    template_missing: AtomicU64,
    parse_errors: AtomicU64,
    bytes: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CountersSnapshot {
    /// Datagrams received.
    pub received: u64,
    /// Flow records decoded.
    pub decoded: u64,
    pub template_missing: u64,
    pub parse_errors: u64,
    /// Bytes represented by decoded flows (sampling-scaled).
    pub bytes: u64,
}

impl ExporterCounters {
    pub fn add_received(&self) {
        self.received.fetch_add(1, Ordering::Relaxed);
    }
    pub fn add_decoded(&self, flows: u64, bytes: u64) {
        self.decoded.fetch_add(flows, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    pub fn add_template_missing(&self, n: u64) {
        self.template_missing.fetch_add(n, Ordering::Relaxed);
    }
    pub fn add_parse_error(&self) {
        self.parse_errors.fetch_add(1, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> CountersSnapshot {
        CountersSnapshot {
            received: self.received.load(Ordering::Relaxed),
            decoded: self.decoded.load(Ordering::Relaxed),
            template_missing: self.template_missing.load(Ordering::Relaxed),
            parse_errors: self.parse_errors.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }
}

/// Exponentially weighted flows/s estimate, updated by the health ticker.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Rate {
    last_total: u64,
    pub per_sec: f64,
}

impl Rate {
    const ALPHA: f64 = 0.5;

    /// Return the updated rate after `elapsed_secs` with a new running total.
    pub fn update(self, total: u64, elapsed_secs: f64) -> Rate {
        if elapsed_secs <= 0.0 {
            return self;
        }
        let inst = total.saturating_sub(self.last_total) as f64 / elapsed_secs;
        let per_sec = if self.last_total == 0 && self.per_sec == 0.0 {
            inst
        } else {
            Self::ALPHA * inst + (1.0 - Self::ALPHA) * self.per_sec
        };
        Rate {
            last_total: total,
            per_sec,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_rates() {
        let g = GlobalCounters::default();
        g.drop_n(DropReason::Backpressure, 3);
        assert_eq!(g.dropped(DropReason::Backpressure), 3);
        assert_eq!(g.snapshot().len(), DropReason::ALL.len());
        let c = ExporterCounters::default();
        c.add_received();
        c.add_decoded(5, 500);
        c.add_template_missing(1);
        c.add_parse_error();
        let s = c.snapshot();
        assert_eq!(
            (
                s.received,
                s.decoded,
                s.bytes,
                s.template_missing,
                s.parse_errors
            ),
            (1, 5, 500, 1, 1)
        );
        let r = Rate::default().update(100, 10.0);
        assert_eq!(r.per_sec, 10.0);
        let r2 = r.update(300, 10.0);
        assert_eq!(r2.per_sec, 15.0);
        assert_eq!(r2.update(300, 0.0), r2);
        assert_eq!(DropReason::NotAllowed.as_str(), "not_allowed");
    }
}
