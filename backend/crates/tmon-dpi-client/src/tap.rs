//! Bounded, never-blocking packet queue to the classifier (FR-018).

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use tmon_capture::{PacketTap, TapDir};
use tmon_core::naming::ConnKey;
use tmon_dpi_proto::{MAX_L3, MIN_L3, Packet, ToClassifier};
use tokio::sync::mpsc;

use crate::convert::{dir, to_wire};
use crate::lru_shards::Sharded;

/// Connections the classifier finished with (no more packets needed).
pub const FINAL_SET_CAPACITY: usize = 100_000;

/// Monotonic tap counters (contracts/http-api.md `queue`).
#[derive(Debug, Default)]
pub struct TapCounters {
    pub sent: AtomicU64,
    pub dropped_queue: AtomicU64,
    pub dropped_down: AtomicU64,
    /// Final connections and connections past `max_packets_per_conn`.
    pub skipped_final: AtomicU64,
}

/// Plain snapshot of [`TapCounters`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TapValues {
    pub sent: u64,
    pub dropped_queue: u64,
    pub dropped_down: u64,
    pub skipped_final: u64,
}

impl TapCounters {
    pub fn values(&self) -> TapValues {
        TapValues {
            sent: self.sent.load(Relaxed),
            dropped_queue: self.dropped_queue.load(Relaxed),
            dropped_down: self.dropped_down.load(Relaxed),
            skipped_final: self.skipped_final.load(Relaxed),
        }
    }
}

pub struct DpiTap {
    tx: mpsc::Sender<Vec<u8>>,
    queued_bytes: AtomicUsize,
    max_bytes: usize,
    running: AtomicBool,
    final_set: Sharded<ConnKey, ()>,
    packets: Sharded<ConnKey, u16>,
    max_packets: u16,
    pub counters: TapCounters,
}

impl DpiTap {
    /// The tap and the receiving end the I/O writer drains.
    pub fn new(
        capacity: usize,
        max_bytes: usize,
        max_packets: u16,
        max_flows: usize,
    ) -> (Self, mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = mpsc::channel(capacity.max(1));
        let tap = Self {
            tx,
            queued_bytes: AtomicUsize::new(0),
            max_bytes,
            running: AtomicBool::new(false),
            final_set: Sharded::new(FINAL_SET_CAPACITY),
            packets: Sharded::new(max_flows.max(1)),
            max_packets,
            counters: TapCounters::default(),
        };
        (tap, rx)
    }

    /// Whether a classifier is connected; offers while down are counted
    /// as `dropped_down` and never queued.
    pub fn set_running(&self, running: bool) {
        self.running.store(running, Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Relaxed)
    }

    /// The classifier will not refine `key` further.
    pub fn mark_final(&self, key: ConnKey) {
        self.final_set.shard(&key).put(key, ());
        self.packets.shard(&key).pop(&key);
    }

    /// Forget per-connection state (after a classifier restart).
    pub fn reset(&self) {
        self.final_set.clear();
        self.packets.clear();
    }

    /// The writer took `len` bytes off the queue.
    pub fn dequeued(&self, len: usize) {
        update(&self.queued_bytes, |q| Some(q.saturating_sub(len)));
    }

    /// Reserve `len` queue bytes; false (nothing reserved) when over budget.
    fn reserve(&self, len: usize) -> bool {
        update(&self.queued_bytes, |q| {
            q.checked_add(len).filter(|n| *n <= self.max_bytes)
        })
    }

    pub fn queued_bytes(&self) -> usize {
        self.queued_bytes.load(Relaxed)
    }

    /// Whether the connection may send another packet.
    fn within_budget(&self, key: &ConnKey) -> bool {
        self.packets
            .shard(key)
            .peek(key)
            .is_none_or(|n| *n < self.max_packets)
    }

    /// Count a queued packet against the per-connection budget (packets
    /// dropped before reaching the queue do not count).
    fn charge(&self, key: &ConnKey) {
        let mut s = self.packets.shard(key);
        let n = s.get_or_insert_mut(*key, || 0);
        *n = n.saturating_add(1);
    }
}

/// Atomically replace the value with `f(current)`; false (unchanged) when
/// `f` returns None. A CAS loop (works on the crate's MSRV).
fn update(a: &AtomicUsize, f: impl Fn(usize) -> Option<usize>) -> bool {
    let mut cur = a.load(Relaxed);
    loop {
        let Some(next) = f(cur) else { return false };
        match a.compare_exchange_weak(cur, next, Relaxed, Relaxed) {
            Ok(_) => return true,
            Err(seen) => cur = seen,
        }
    }
}

impl PacketTap for DpiTap {
    fn offer(&self, key: &ConnKey, d: TapDir, ts_ms: i64, l3: &[u8]) {
        let c = &self.counters;
        if self.final_set.shard(key).contains(key) || !self.within_budget(key) {
            c.skipped_final.fetch_add(1, Relaxed);
            return;
        }
        if !self.is_running() {
            c.dropped_down.fetch_add(1, Relaxed);
            return;
        }
        if !(MIN_L3..=MAX_L3).contains(&l3.len()) {
            return;
        }
        let msg = ToClassifier::Packet(Packet {
            key: to_wire(key),
            dir: dir(d),
            ts_ms: u64::try_from(ts_ms).unwrap_or(0),
            l3: l3.to_vec(),
        })
        .encode();
        // Reserve before sending: the writer may dequeue (and subtract)
        // before try_send even returns.
        let len = msg.len();
        if !self.reserve(len) {
            c.dropped_queue.fetch_add(1, Relaxed);
            return;
        }
        match self.tx.try_send(msg) {
            Ok(()) => {
                self.charge(key);
                c.sent.fetch_add(1, Relaxed);
            }
            Err(_) => {
                self.dequeued(len);
                c.dropped_queue.fetch_add(1, Relaxed);
            }
        }
    }
}
