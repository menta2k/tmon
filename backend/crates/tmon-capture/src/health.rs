//! Capture counters (data-model.md §4) and the 15-minute window the health
//! detectors read (R10). Counters only ever increase; snapshots are values.

use std::sync::Mutex;
use std::time::Duration;

use crate::listener::CaptureStats;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed};

/// Detector window: 15 one-minute buckets.
pub const WINDOW_MINUTES: usize = 15;
const MINUTE_MS: i64 = 60_000;

macro_rules! counters {
    ($($name:ident),* $(,)?) => {
        /// Monotonic counters, shared by the listener, processor and table.
        #[derive(Debug, Default)]
        pub struct Counters { $(pub $name: AtomicU64,)* }

        /// Point-in-time copy of [`Counters`].
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct CounterValues { $(pub $name: u64,)* }

        impl Counters {
            pub fn values(&self) -> CounterValues {
                CounterValues { $($name: self.$name.load(Relaxed),)* }
            }
        }
    };
}

counters!(
    datagrams_received,
    bytes_received,
    rejected_source,
    dropped_backpressure,
    unsupported_tzsp,
    unsupported_encap,
    unsupported_l3,
    unsupported_fragment,
    malformed_tzsp,
    malformed_frame,
    tls_started,
    tls_named,
    tls_incomplete,
    tls_parse_error,
    tls_ech,
    quic_started,
    quic_named,
    quic_unsupported_version,
    quic_decrypt_error,
    quic_incomplete,
    http_named,
    dns_responses,
    dns_bindings,
    dns_parse_error,
    name_invalid,
    tcp_syn_seen,
    tcp_payload_first_seen,
    conn_evicted_lru,
    conn_evicted_ttl,
    conn_evicted_bytes,
    bindings_evicted,
    lookups,
    lookup_hits_conn,
    lookup_hits_dns,
);

impl CounterValues {
    pub fn unsupported(&self) -> u64 {
        self.unsupported_tzsp
            + self.unsupported_encap
            + self.unsupported_l3
            + self.unsupported_fragment
    }

    pub fn malformed(&self) -> u64 {
        self.malformed_tzsp + self.malformed_frame
    }
}

pub fn inc(c: &AtomicU64) {
    c.fetch_add(1, Relaxed);
}

pub fn add(c: &AtomicU64, n: u64) {
    c.fetch_add(n, Relaxed);
}

/// Per-minute activity the detectors need.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bucket {
    pub minute: i64,
    pub tls_started: u64,
    pub tls_incomplete: u64,
    pub tcp_syn: u64,
    pub tcp_payload_first: u64,
    pub max_datagram_len: u64,
    pub lookups: u64,
    pub lookup_hits: u64,
}

impl Bucket {
    fn merge(self, o: &Bucket) -> Bucket {
        Bucket {
            minute: self.minute.max(o.minute),
            tls_started: self.tls_started + o.tls_started,
            tls_incomplete: self.tls_incomplete + o.tls_incomplete,
            tcp_syn: self.tcp_syn + o.tcp_syn,
            tcp_payload_first: self.tcp_payload_first + o.tcp_payload_first,
            max_datagram_len: self.max_datagram_len.max(o.max_datagram_len),
            lookups: self.lookups + o.lookups,
            lookup_hits: self.lookup_hits + o.lookup_hits,
        }
    }
}

/// Ring of [`WINDOW_MINUTES`] buckets; no history beyond it is kept.
#[derive(Debug, Clone, Default)]
pub struct Window {
    buckets: [Bucket; WINDOW_MINUTES],
}

impl Window {
    fn bucket(&mut self, now_ms: i64) -> &mut Bucket {
        let minute = now_ms.div_euclid(MINUTE_MS);
        let b = &mut self.buckets[minute.rem_euclid(WINDOW_MINUTES as i64) as usize];
        if b.minute != minute {
            *b = Bucket {
                minute,
                ..Bucket::default()
            };
        }
        b
    }

    /// Sum of the buckets within the last [`WINDOW_MINUTES`] minutes.
    pub fn total(&self, now_ms: i64) -> Bucket {
        let minute = now_ms.div_euclid(MINUTE_MS);
        let oldest = minute - WINDOW_MINUTES as i64;
        self.buckets
            .iter()
            .filter(|b| b.minute > oldest && b.minute <= minute)
            .fold(Bucket::default(), |acc, b| acc.merge(b))
    }
}

/// Counters + window + last-datagram time.
#[derive(Debug, Default)]
pub struct Health {
    pub counters: Counters,
    window: Mutex<Window>,
    /// 0 = never.
    last_datagram_ms: AtomicI64,
}

impl Health {
    /// Update the current minute's bucket.
    pub fn record(&self, now_ms: i64, f: impl FnOnce(&mut Bucket)) {
        let mut w = self.window.lock().unwrap_or_else(|e| e.into_inner());
        f(w.bucket(now_ms));
    }

    /// An accepted datagram of `len` bytes.
    pub fn datagram(&self, now_ms: i64, len: usize) {
        inc(&self.counters.datagrams_received);
        add(&self.counters.bytes_received, len as u64);
        self.last_datagram_ms.fetch_max(now_ms, Relaxed);
        self.record(now_ms, |b| {
            b.max_datagram_len = b.max_datagram_len.max(len as u64)
        });
    }

    pub fn last_datagram_ms(&self) -> Option<i64> {
        Some(self.last_datagram_ms.load(Relaxed)).filter(|t| *t > 0)
    }

    pub fn window(&self, now_ms: i64) -> Bucket {
        self.window
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .total(now_ms)
    }
}

// ---------- detectors (R10, FR-019..FR-021) ----------

/// Large-packet loss: at least this many TLS starts in the window…
pub const LARGE_LOSS_MIN_TLS: u64 = 50;
/// …of which at least this share never completed…
pub const LARGE_LOSS_MIN_INCOMPLETE_RATIO: f64 = 0.5;
/// …and no datagram larger than this arrived (WSL2 mirrored networking).
pub const LARGE_LOSS_MAX_DATAGRAM: u64 = 1472;
/// Fast-path bypass: at least this many SYNs in the window…
pub const FASTPATH_MIN_SYNS: u64 = 100;
/// …and fewer than this share had their first payload captured.
pub const FASTPATH_MAX_PAYLOAD_RATIO: f64 = 0.1;

/// A capture problem the operator should fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureWarning {
    /// The capture socket could not be bound.
    ListenerDown { port: u16 },
    /// Enabled, but no datagram for `silent_after`.
    Silent,
    /// Datagrams over 1,472 bytes are being dropped on the way to tmon.
    LargePacketsLost,
    /// Connections are seen but their payload is not (FastTrack).
    FastpathSuspected,
}

impl CaptureWarning {
    pub fn code(&self) -> String {
        match self {
            Self::ListenerDown { port } => format!("collector_down:{port}"),
            Self::Silent => "capture_silent".into(),
            Self::LargePacketsLost => "capture_large_packets_lost".into(),
            Self::FastpathSuspected => "capture_fastpath_suspected".into(),
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::ListenerDown { port } => {
                format!("The packet-capture listener on UDP port {port} could not start.")
            }
            Self::Silent => "Capture is enabled but no packets arrived in 5 min. Check the router \
                             mangle rules (see setup guide)."
                .into(),
            Self::LargePacketsLost => "Large capture packets are being lost (e.g. WSL2 mirrored \
                                       networking). Run tmon natively or in Docker."
                .into(),
            Self::FastpathSuspected => "Connections are seen but their setup packets are not. \
                                        Exempt the first 6 KB from FastTrack."
                .into(),
        }
    }
}

fn ratio(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn is_silent(s: &CaptureStats, silent_after: Duration, now_ms: i64) -> bool {
    let since = s.last_datagram_ms.unwrap_or(s.started_ms);
    now_ms - since > silent_after.as_millis() as i64
}

fn large_packets_lost(w: &Bucket) -> bool {
    w.tls_started >= LARGE_LOSS_MIN_TLS
        && ratio(w.tls_incomplete, w.tls_started) >= LARGE_LOSS_MIN_INCOMPLETE_RATIO
        && w.max_datagram_len <= LARGE_LOSS_MAX_DATAGRAM
}

fn fastpath_suspected(w: &Bucket) -> bool {
    w.tcp_syn >= FASTPATH_MIN_SYNS
        && ratio(w.tcp_payload_first, w.tcp_syn) < FASTPATH_MAX_PAYLOAD_RATIO
}

/// Current capture warnings for a stats snapshot.
pub fn warnings(s: &CaptureStats, silent_after: Duration, now_ms: i64) -> Vec<CaptureWarning> {
    if !s.listening {
        return vec![CaptureWarning::ListenerDown {
            port: s.address.port(),
        }];
    }
    let checks = [
        (is_silent(s, silent_after, now_ms), CaptureWarning::Silent),
        (
            large_packets_lost(&s.window),
            CaptureWarning::LargePacketsLost,
        ),
        (
            fastpath_suspected(&s.window),
            CaptureWarning::FastpathSuspected,
        ),
    ];
    checks
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, w)| w)
        .collect()
}
