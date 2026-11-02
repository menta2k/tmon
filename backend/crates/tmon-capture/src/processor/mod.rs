//! Per-datagram dispatch (single task, owns all reassembly state).
//!
//! Only names reach the shared [`ConnTable`]; payload bytes live here, bounded
//! per connection and in total, and are dropped as soon as a connection is
//! named, failed or expired (FR-014, FR-016).

mod tcp;
mod udp;

use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tmon_core::naming::{ConnKey, ConnName, NameSource};

use crate::frame::{self, Packet, Reject, Transport};
use crate::health::{Health, inc};
use crate::pending::PendingRemoteNames;
use crate::quic::InitialKeys;
use crate::settings::Limits;
use crate::table::ConnTable;
use crate::tap::{TapConfig, TapDir};
use crate::tcp_reasm::TcpReassembly;

/// How often expired reassembly state is swept.
const SWEEP_EVERY_MS: i64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TcpKind {
    Unknown,
    Tls,
    Http,
}

enum Pending {
    Tcp {
        reasm: TcpReassembly,
        kind: TcpKind,
        started_ms: i64,
    },
    Quic {
        keys: Box<InitialKeys>,
        reasm: TcpReassembly,
        started_ms: i64,
    },
}

impl Pending {
    fn bytes(&self) -> usize {
        match self {
            Pending::Tcp { reasm, .. } | Pending::Quic { reasm, .. } => reasm.bytes(),
        }
    }

    fn started_ms(&self) -> i64 {
        match self {
            Pending::Tcp { started_ms, .. } | Pending::Quic { started_ms, .. } => *started_ms,
        }
    }
}

/// A connection that needs no more work.
#[derive(Debug, Clone, Copy)]
struct Done {
    at_ms: i64,
    /// ISN of a TCP connection, so a retransmitted SYN does not reopen it.
    isn: Option<u32>,
}

impl Pending {
    fn isn(&self) -> Option<u32> {
        match self {
            Pending::Tcp { reasm, .. } => Some(reasm.base_seq().wrapping_sub(1)),
            Pending::Quic { .. } => None,
        }
    }
}

/// Shared outputs of the processor.
#[derive(Clone)]
pub struct Outputs {
    pub table: Arc<ConnTable>,
    pub health: Arc<Health>,
    pub names: Arc<PendingRemoteNames>,
}

pub struct CaptureProcessor {
    out: Outputs,
    limits: Limits,
    /// ISN of connections whose SYN was seen but no payload yet.
    syns: LruCache<ConnKey, u32>,
    pending: LruCache<ConnKey, Pending>,
    /// Named or given up: later packets are ignored until a new SYN (a
    /// different ISN) or until `conn_ttl_ms` has passed.
    done: LruCache<ConnKey, Done>,
    /// Time of the datagram being processed.
    now_ms: i64,
    pending_bytes: usize,
    last_sweep_ms: i64,
    /// Optional classifier tap (feature 003); also enables orienting reply
    /// packets by the local networks.
    tap: Option<TapConfig>,
}

fn cap(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n.max(1)).expect("non-zero")
}

impl CaptureProcessor {
    pub fn new(out: Outputs, limits: Limits) -> Self {
        let n = cap(limits.max_connections);
        Self {
            out,
            limits,
            syns: LruCache::new(n),
            pending: LruCache::new(n),
            done: LruCache::new(n),
            pending_bytes: 0,
            last_sweep_ms: 0,
            now_ms: 0,
            tap: None,
        }
    }

    /// Forward decoded packets to `tap` (feature 003).
    pub fn with_tap(self, tap: TapConfig) -> Self {
        Self {
            tap: Some(tap),
            ..self
        }
    }

    /// Bytes currently held for reassembly.
    pub fn reassembly_bytes(&self) -> usize {
        self.pending_bytes
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Process one accepted capture datagram.
    pub fn handle(&mut self, datagram: &[u8], now_ms: i64) {
        self.now_ms = now_ms;
        self.out.health.datagram(now_ms, datagram.len());
        match frame::decode(datagram) {
            Ok(p) => self.dispatch(&p, now_ms),
            Err(r) => self.count_reject(r),
        }
        if now_ms - self.last_sweep_ms >= SWEEP_EVERY_MS {
            self.sweep(now_ms);
        }
    }

    fn dispatch(&mut self, p: &Packet<'_>, now_ms: i64) {
        if matches!(p.transport, Transport::Udp) && p.src_port == udp::DNS_PORT {
            return self.dns(p, now_ms);
        }
        let (key, dir) = self.orient(p);
        if let Some(t) = &self.tap
            && key.remote_port != udp::DNS_PORT
        {
            t.tap.offer(&key, dir, now_ms, p.l3);
        }
        if dir == TapDir::RemoteToLan {
            // Server replies only feed the classifier; naming uses the
            // client's setup messages.
            return;
        }
        match p.transport {
            Transport::Tcp(t) => self.tcp(key, t, p.payload, now_ms),
            Transport::Udp => self.quic(key, p.payload, now_ms),
        }
    }

    /// LAN-side key. Without a tap the source is the LAN side (feature 002
    /// captures LAN → WAN only); with one, a packet from a remote address to
    /// a local one is a reply and its key is reversed.
    fn orient(&self, p: &Packet<'_>) -> (ConnKey, TapDir) {
        let reply = self
            .tap
            .as_ref()
            .is_some_and(|t| !(t.is_local)(&p.src_ip) && (t.is_local)(&p.dst_ip));
        let key = |lan_ip, lan_port, remote_ip, remote_port| ConnKey {
            lan_ip,
            lan_port,
            remote_ip,
            remote_port,
            proto: p.proto(),
        };
        if reply {
            let k = key(p.dst_ip, p.dst_port, p.src_ip, p.src_port);
            (k, TapDir::RemoteToLan)
        } else {
            let k = key(p.src_ip, p.src_port, p.dst_ip, p.dst_port);
            (k, TapDir::LanToRemote)
        }
    }

    fn count_reject(&self, r: Reject) {
        let c = &self.out.health.counters;
        inc(match r {
            Reject::MalformedTzsp => &c.malformed_tzsp,
            Reject::UnsupportedTzsp => &c.unsupported_tzsp,
            Reject::UnsupportedEncap => &c.unsupported_encap,
            Reject::UnsupportedL3 => &c.unsupported_l3,
            Reject::Fragment => &c.unsupported_fragment,
            Reject::MalformedFrame => &c.malformed_frame,
        });
    }

    /// Track a new pending connection, evicting old state when over limits.
    fn start(&mut self, key: ConnKey, p: Pending) {
        self.pending_bytes += p.bytes();
        if let Some((old_key, old)) = self.pending.push(key, p)
            && old_key != key
        {
            self.pending_bytes -= old.bytes();
            inc(&self.out.health.counters.conn_evicted_lru);
            self.mark_done(old_key, &old);
        }
    }

    /// Apply a byte delta for `key` and enforce the global cap.
    fn account(&mut self, key: &ConnKey, delta: isize) {
        self.pending_bytes = self.pending_bytes.saturating_add_signed(delta);
        while self.pending_bytes > self.limits.max_reassembly_bytes_total {
            let Some((old_key, old)) = self.pending.pop_lru() else {
                break;
            };
            self.pending_bytes -= old.bytes();
            inc(&self.out.health.counters.conn_evicted_bytes);
            self.mark_done(old_key, &old);
            if old_key == *key {
                break;
            }
        }
    }

    /// Stop tracking `key`; publish `name` if one was found.
    fn finish(&mut self, key: ConnKey, name: Option<ConnName>, now_ms: i64) {
        let isn = self.pending.pop(&key).and_then(|p| {
            self.pending_bytes -= p.bytes();
            p.isn()
        });
        self.done.put(key, Done { at_ms: now_ms, isn });
        if let Some(n) = name {
            self.out.table.insert(key, Arc::new(n), now_ms);
        }
    }

    fn mark_done(&mut self, key: ConnKey, p: &Pending) {
        let d = Done {
            at_ms: self.now_ms,
            isn: p.isn(),
        };
        self.done.put(key, d);
    }

    /// Finished and not yet expired (expired entries are dropped).
    fn is_done(&mut self, key: &ConnKey, now_ms: i64) -> bool {
        match self.done.get(key) {
            Some(d) if d.at_ms + self.limits.conn_ttl_ms >= now_ms => true,
            Some(_) => {
                self.done.pop(key);
                false
            }
            None => false,
        }
    }

    /// A new SYN: reopen the tuple unless this is a retransmit of the SYN
    /// of a connection already finished or in progress.
    fn reopen_on_syn(&mut self, key: &ConnKey, isn: u32) -> bool {
        let same = |i: Option<u32>| i == Some(isn);
        if self.done.peek(key).is_some_and(|d| same(d.isn))
            || self.pending.peek(key).is_some_and(|p| same(p.isn()))
        {
            return false;
        }
        self.done.pop(key);
        if let Some(p) = self.pending.pop(key) {
            self.pending_bytes -= p.bytes();
        }
        true
    }

    /// Give up on reassemblies older than the timeout.
    pub fn sweep(&mut self, now_ms: i64) {
        self.last_sweep_ms = now_ms;
        let timeout = self.limits.reassembly_timeout_ms;
        let expired: Vec<ConnKey> = self
            .pending
            .iter()
            .filter(|(_, p)| p.started_ms() + timeout < now_ms)
            .map(|(k, _)| *k)
            .collect();
        for key in expired {
            self.incomplete(&key, now_ms);
            self.finish(key, None, now_ms);
        }
    }

    fn incomplete(&self, key: &ConnKey, now_ms: i64) {
        let c = &self.out.health.counters;
        match self.pending.peek(key) {
            Some(Pending::Tcp {
                kind: TcpKind::Tls, ..
            }) => {
                inc(&c.tls_incomplete);
                self.out.health.record(now_ms, |b| b.tls_incomplete += 1);
            }
            Some(Pending::Quic { .. }) => inc(&c.quic_incomplete),
            _ => {}
        }
    }
}

/// Build a name from an extracted host, counting invalid ones.
fn make_name(health: &Health, host: &str, source: NameSource) -> Option<ConnName> {
    let n = ConnName::new(host, source);
    if n.is_err() {
        inc(&health.counters.name_invalid);
    }
    n.ok()
}
