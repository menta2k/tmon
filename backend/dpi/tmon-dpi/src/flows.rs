//! Per-connection classifier state (data-model.md §4): an LRU of flows,
//! idle sweep, memory budget, "report on change" and `final` handling.

use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use tmon_dpi_proto::{Classification, Key, MAX_TOP, Packet, Stats, TopApp};

use crate::classify::{Verdict, classify};
use crate::engine::{Engine, NdpiResult};

/// Distinct app names kept for `top_apps` (bounded; nDPI knows ~450).
const MAX_TOP_TRACKED: usize = 2_048;
/// Below this share of the budget, memory eviction stops (research R9).
const MEMORY_LOW_WATER: f64 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_flows: usize,
    pub max_packets: u16,
    pub idle_timeout_ms: u64,
    pub mem_limit_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub packets_in: u64,
    pub packets_invalid: u64,
    pub flows_created: u64,
    pub evicted_lru: u64,
    pub evicted_idle: u64,
    pub evicted_memory: u64,
    pub classified_confident: u64,
    pub classified_guess: u64,
    pub giveups: u64,
}

struct DpiFlow<F> {
    state: F,
    packets: u16,
    reported: Option<(String, bool)>,
    last_seen_ms: u64,
}

/// Field order matters: the per-flow engine state in `table` is dropped
/// before `engine` (the nDPI module it was created from).
pub struct Flows<E: Engine> {
    table: LruCache<Key, DpiFlow<E::Flow>>,
    /// Keys already reported `final`: later packets are ignored.
    done: LruCache<Key, u64>,
    limits: Limits,
    /// Engine memory before any flow (the detection module itself); the
    /// budget applies to what flows add on top.
    baseline: u64,
    counters: Counters,
    top: HashMap<String, u32>,
    engine: E,
}

impl<E: Engine> Flows<E> {
    pub fn new(engine: E, limits: Limits) -> Self {
        let cap = NonZeroUsize::new(limits.max_flows.max(1)).expect("non-zero");
        Self {
            baseline: engine.mem_bytes(),
            engine,
            limits,
            table: LruCache::new(cap),
            done: LruCache::new(cap),
            counters: Counters::default(),
            top: HashMap::new(),
        }
    }

    pub fn counters(&self) -> Counters {
        self.counters
    }

    pub fn active(&self) -> usize {
        self.table.len()
    }

    pub fn engine(&self) -> &E {
        &self.engine
    }

    pub fn invalid(&mut self) {
        self.counters.packets_invalid += 1;
    }

    /// Feed a packet; returns a Classification when the verdict changed or
    /// the flow became final.
    pub fn packet(&mut self, p: &Packet) -> Option<Classification> {
        self.counters.packets_in += 1;
        if self.done.get(&p.key).is_some() {
            return None;
        }
        if !self.ensure(p.key, p.ts_ms) {
            self.counters.packets_invalid += 1;
            return None;
        }
        let flow = self.table.get_mut(&p.key)?;
        flow.packets = flow.packets.saturating_add(1);
        flow.last_seen_ms = p.ts_ms;
        let r = self.engine.process(&mut flow.state, &p.l3, p.ts_ms);
        let out = self.settle(p.key, r);
        self.enforce_budget();
        out
    }

    /// Make sure a flow exists for `key` (evicting the LRU entry when
    /// full); false when the engine could not allocate one.
    fn ensure(&mut self, key: Key, now: u64) -> bool {
        if !self.table.contains(&key) {
            let Some(state) = self.engine.new_flow() else {
                return false;
            };
            let fresh = DpiFlow {
                state,
                packets: 0,
                reported: None,
                last_seen_ms: now,
            };
            if let Some((old, _)) = self.table.push(key, fresh) {
                if old != key {
                    self.counters.evicted_lru += 1;
                }
            }
            self.counters.flows_created += 1;
        }
        true
    }

    /// Decide what to report after a packet, and finish the flow if final.
    fn settle(&mut self, key: Key, r: NdpiResult) -> Option<Classification> {
        let flow = self.table.get_mut(&key)?;
        let mut verdict = classify(&r);
        let exhausted = flow.packets >= self.limits.max_packets;
        let mut final_ = verdict.as_ref().is_some_and(|v| v.confident);
        if !final_ && exhausted {
            self.counters.giveups += 1;
            let g = self.engine.giveup(&mut flow.state);
            verdict = classify(&g).or(verdict);
            final_ = true;
        }
        let changed = verdict
            .as_ref()
            .is_some_and(|v| flow.reported.as_ref() != Some(&(v.app.clone(), v.confident)));
        if let Some(v) = &verdict {
            flow.reported = Some((v.app.clone(), v.confident));
        }
        if final_ {
            self.table.pop(&key);
            self.done.put(key, 0);
        }
        let v = verdict?;
        if !(changed || final_) {
            return None;
        }
        self.count(&v, changed);
        Some(Classification {
            key,
            app: v.app,
            category: v.category,
            confident: v.confident,
            final_,
            host: v.host,
        })
    }

    fn count(&mut self, v: &Verdict, new_verdict: bool) {
        if !new_verdict {
            return;
        }
        if v.confident {
            self.counters.classified_confident += 1;
        } else {
            self.counters.classified_guess += 1;
        }
        if self.top.len() < MAX_TOP_TRACKED || self.top.contains_key(&v.app) {
            *self.top.entry(v.app.clone()).or_default() += 1;
        }
    }

    /// Free LRU flows while the engine is over budget (research R9).
    fn enforce_budget(&mut self) {
        let limit = self.limits.mem_limit_bytes;
        let used = |e: &E| e.mem_bytes().saturating_sub(self.baseline);
        if limit == 0 || used(&self.engine) <= limit {
            return;
        }
        let low = (limit as f64 * MEMORY_LOW_WATER) as u64;
        while used(&self.engine) > low {
            if self.table.pop_lru().is_none() {
                break;
            }
            self.counters.evicted_memory += 1;
        }
    }

    /// Drop the flow for `key` (tmon no longer needs it).
    pub fn forget(&mut self, key: &Key) {
        self.table.pop(key);
        self.done.pop(key);
    }

    /// Free flows idle for longer than the timeout.
    pub fn sweep(&mut self, now_ms: u64) {
        let idle = self.limits.idle_timeout_ms;
        while let Some((_, f)) = self.table.peek_lru() {
            if f.last_seen_ms.saturating_add(idle) > now_ms {
                break;
            }
            self.table.pop_lru();
            self.counters.evicted_idle += 1;
        }
    }

    /// The heartbeat / statistics message.
    pub fn stats(&self, uptime_ms: u64) -> Stats {
        let c = self.counters;
        let mut top: Vec<TopApp> = self
            .top
            .iter()
            .map(|(app, flows)| TopApp {
                app: app.clone(),
                flows: *flows,
            })
            .collect();
        top.sort_by(|a, b| b.flows.cmp(&a.flows).then_with(|| a.app.cmp(&b.app)));
        top.truncate(MAX_TOP);
        Stats {
            uptime_ms,
            packets_in: c.packets_in,
            packets_invalid: c.packets_invalid,
            flows_active: u32::try_from(self.table.len()).unwrap_or(u32::MAX),
            flows_created: c.flows_created,
            evicted_lru: c.evicted_lru,
            evicted_idle: c.evicted_idle,
            evicted_memory: c.evicted_memory,
            classified_confident: c.classified_confident,
            classified_guess: c.classified_guess,
            giveups: c.giveups,
            // Flow memory only: the quantity `mem_limit_bytes` limits.
            mem_bytes: self.engine.mem_bytes().saturating_sub(self.baseline),
            mem_limit_bytes: self.limits.mem_limit_bytes,
            top,
        }
    }
}
