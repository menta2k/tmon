//! Collection health (FR-019): flow rates and stall warnings.

use arc_swap::ArcSwap;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::counters::Rate;
use crate::exporter_registry::{ExporterInfo, ExporterRegistry, ExporterState};

pub const HEALTH_TICK: Duration = Duration::from_secs(10);

/// Immutable warning shown as a banner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Warning {
    pub code: String,
    pub message: String,
}

impl Warning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

fn duration_text(ms: i64) -> String {
    if ms >= 60_000 {
        format!("{} min", ms / 60_000)
    } else {
        format!("{} s", ms / 1000)
    }
}

/// One warning per stalled exporter, e.g. "No flows received from 192.168.88.1 for 5 min".
pub fn stall_warnings(exporters: &[ExporterInfo], now_ms: i64, stall_ms: i64) -> Vec<Warning> {
    exporters
        .iter()
        .filter(|e| e.state == ExporterState::Stalled)
        .map(|e| {
            Warning::new(
                format!("exporter_stalled:{}", e.id),
                format!(
                    "No flows received from {} for {}",
                    e.address,
                    duration_text(stall_ms.max(now_ms - e.last_seen))
                ),
            )
        })
        .collect()
}

/// Rates computed on each tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rates {
    pub total: Rate,
    pub per_exporter: BTreeMap<u16, Rate>,
}

impl Rates {
    pub fn flows_per_sec(&self) -> f64 {
        self.total.per_sec
    }

    pub fn exporter(&self, id: u16) -> f64 {
        self.per_exporter.get(&id).map_or(0.0, |r| r.per_sec)
    }

    /// Next rates from the registry's decoded counters.
    pub fn next(&self, registry: &ExporterRegistry, elapsed_secs: f64) -> Rates {
        let mut per_exporter = BTreeMap::new();
        let mut total = 0;
        for e in registry.all() {
            let decoded = e.counters.snapshot().decoded;
            total += decoded;
            per_exporter.insert(
                e.id,
                self.per_exporter
                    .get(&e.id)
                    .copied()
                    .unwrap_or_default()
                    .update(decoded, elapsed_secs),
            );
        }
        Rates {
            total: self.total.update(total, elapsed_secs),
            per_exporter,
        }
    }
}

/// Periodically updated flow-rate estimates.
pub struct HealthMonitor {
    registry: Arc<ExporterRegistry>,
    rates: ArcSwap<Rates>,
}

impl HealthMonitor {
    pub fn new(registry: Arc<ExporterRegistry>) -> Arc<Self> {
        Arc::new(Self {
            registry,
            rates: ArcSwap::from_pointee(Rates::default()),
        })
    }

    pub fn rates(&self) -> Arc<Rates> {
        self.rates.load_full()
    }

    pub fn tick(&self, elapsed_secs: f64) {
        self.rates.store(Arc::new(
            self.rates.load().next(&self.registry, elapsed_secs),
        ));
    }

    pub fn spawn(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let mut t = tokio::time::interval(HEALTH_TICK);
            t.tick().await;
            loop {
                t.tick().await;
                me.tick(HEALTH_TICK.as_secs_f64());
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exporter_registry::FlowProtocol;

    #[test]
    fn stalled_exporters_produce_warnings_and_rates_track_decoded() {
        let reg = Arc::new(ExporterRegistry::new());
        let (e, _) = reg
            .get_or_create(
                "192.168.88.1".parse().unwrap(),
                0,
                FlowProtocol::Ipfix,
                1_000,
            )
            .unwrap();
        e.counters.add_decoded(100, 1000);
        let mon = HealthMonitor::new(reg.clone());
        mon.tick(10.0);
        assert_eq!(mon.rates().flows_per_sec(), 10.0);
        assert_eq!(mon.rates().exporter(e.id), 10.0);
        let w = stall_warnings(
            &reg.snapshot(1_000 + 400_000, 300_000),
            1_000 + 400_000,
            300_000,
        );
        assert_eq!(w.len(), 1);
        assert_eq!(
            w[0].message,
            "No flows received from 192.168.88.1 for 6 min"
        );
        assert!(stall_warnings(&reg.snapshot(2_000, 300_000), 2_000, 300_000).is_empty());
    }
}
