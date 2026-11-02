//! Per-interval live totals published to SSE subscribers (FR-016).

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tmon_core::{Direction, FlowRecord};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::pipeline::BatchHook;

pub const TICK: Duration = Duration::from_secs(5);
const CHANNEL: usize = 16;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Rates {
    pub down: f64,
    pub up: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CategoryRate {
    pub id: u16,
    pub bps: f64,
}

/// One `event: tick` payload; rates are bits per second.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tick {
    pub ts: i64,
    pub bytes_per_sec: Rates,
    pub by_category: Vec<CategoryRate>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Acc {
    down: u64,
    up: u64,
    by_cat: BTreeMap<u16, u64>,
}

impl Acc {
    fn add(&self, flows: &[FlowRecord]) -> Acc {
        let mut next = self.clone();
        for f in flows {
            match f.direction {
                Direction::Down => next.down += f.bytes,
                Direction::Up => next.up += f.bytes,
                _ => {}
            }
            *next.by_cat.entry(f.category_id).or_default() += f.bytes;
        }
        next
    }

    fn tick(&self, ts: i64, secs: f64) -> Tick {
        let rate = |b: u64| b as f64 / secs;
        let mut by_category: Vec<CategoryRate> = self
            .by_cat
            .iter()
            .map(|(id, b)| CategoryRate {
                id: *id,
                bps: rate(*b) * 8.0,
            })
            .collect();
        by_category.sort_by(|a, b| b.bps.total_cmp(&a.bps));
        Tick {
            ts,
            bytes_per_sec: Rates {
                down: rate(self.down),
                up: rate(self.up),
            },
            by_category,
        }
    }
}

/// Accumulates enriched batches and emits a `Tick` per interval.
pub struct LiveTap {
    acc: Mutex<Acc>,
    tx: broadcast::Sender<Tick>,
    /// Cumulative bytes per (category, direction) for `/metrics` (bounded label set).
    totals: Mutex<BTreeMap<(u16, Direction), u64>>,
}

impl LiveTap {
    pub fn new() -> Arc<Self> {
        let (tx, _) = broadcast::channel(CHANNEL);
        Arc::new(Self {
            acc: Mutex::new(Acc::default()),
            tx,
            totals: Mutex::new(BTreeMap::new()),
        })
    }

    /// Snapshot of cumulative bytes per (category, direction).
    pub fn totals(&self) -> Vec<((u16, Direction), u64)> {
        self.totals
            .lock()
            .map(|t| t.iter().map(|(k, v)| (*k, *v)).collect())
            .unwrap_or_default()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Tick> {
        self.tx.subscribe()
    }

    pub fn record(&self, flows: &[FlowRecord]) {
        if let Ok(mut acc) = self.acc.lock() {
            *acc = acc.add(flows);
        } else {
            tracing::error!("live accumulator poisoned");
        }
        if let Ok(mut t) = self.totals.lock() {
            for f in flows {
                *t.entry((f.category_id, f.direction)).or_default() += f.bytes;
            }
        }
    }

    /// Take the current totals as a tick and reset.
    pub fn flush(&self, ts: i64, interval: Duration) -> Tick {
        let acc = self
            .acc
            .lock()
            .map(|mut a| std::mem::take(&mut *a))
            .unwrap_or_default();
        let tick = acc.tick(ts, interval.as_secs_f64().max(0.001));
        let _ = self.tx.send(tick.clone()); // no subscribers is fine
        tick
    }

    pub fn hook(self: &Arc<Self>) -> BatchHook {
        let me = Arc::clone(self);
        Arc::new(move |flows| me.record(flows))
    }

    /// Emit a tick every `interval` until the runtime shuts down.
    pub fn spawn(
        self: &Arc<Self>,
        interval: Duration,
        clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> JoinHandle<()> {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let mut t = tokio::time::interval(interval);
            t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            t.tick().await;
            loop {
                t.tick().await;
                me.flush(clock(), interval);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmon_core::RawFlow;

    fn rec(dir: Direction, cat: u16, bytes: u64) -> FlowRecord {
        let raw = RawFlow {
            ts_ms: 0,
            ts_start_ms: 0,
            exporter_id: 1,
            in_if: 0,
            out_if: 0,
            src_ip: "10.0.0.1".parse().unwrap(),
            dst_ip: "1.1.1.1".parse().unwrap(),
            src_port: 1,
            dst_port: 2,
            proto: 6,
            tcp_flags: 0,
            bytes,
            packets: 1,
            sampled: false,
            nat_ip: None,
            nat_port: None,
            nat_dst_ip: None,
            nat_dst_port: None,
            flow_direction: None,
        };
        FlowRecord::from_raw(&raw, dir, true).with_category(cat, None)
    }

    #[tokio::test]
    async fn ticks_aggregate_and_reset() {
        let tap = LiveTap::new();
        let mut rx = tap.subscribe();
        tap.hook()(&[
            rec(Direction::Down, 1, 5000),
            rec(Direction::Up, 2, 1000),
            rec(Direction::Down, 2, 5000),
        ]);
        let t = tap.flush(42, Duration::from_secs(5));
        assert_eq!(t.ts, 42);
        assert_eq!(
            t.bytes_per_sec,
            Rates {
                down: 2000.0,
                up: 200.0
            }
        );
        assert_eq!(t.by_category[0], CategoryRate { id: 2, bps: 9600.0 });
        assert_eq!(rx.recv().await.unwrap(), t);
        assert!(tap.flush(43, Duration::from_secs(5)).by_category.is_empty());
    }
}
