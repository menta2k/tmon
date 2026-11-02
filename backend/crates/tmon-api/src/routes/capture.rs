//! `/status.capture` (feature 002, FR-018): a stable view of the capture
//! counters, independent of the internal counter names.

use serde::Serialize;
use tmon_capture::{CaptureHandle, CaptureStats};

#[derive(Serialize)]
pub struct Datagrams {
    pub received: u64,
    pub rejected_source: u64,
    pub dropped_backpressure: u64,
    pub unsupported: u64,
    pub malformed: u64,
    pub max_len_15m: u64,
}

#[derive(Serialize)]
pub struct Connections {
    pub tracked: u64,
    pub evicted: u64,
    pub reassembly_bytes: u64,
}

#[derive(Serialize)]
pub struct Bindings {
    pub tracked: u64,
    pub evicted: u64,
}

#[derive(Serialize)]
pub struct Names {
    pub tls: u64,
    pub quic: u64,
    pub http: u64,
    pub dns: u64,
    pub ech: u64,
}

#[derive(Serialize)]
pub struct Errors {
    pub tls_incomplete: u64,
    pub tls_parse: u64,
    pub quic_unsupported_version: u64,
    pub quic_decrypt: u64,
    pub quic_incomplete: u64,
    pub dns_parse: u64,
    pub name_invalid: u64,
}

#[derive(Serialize)]
pub struct Enabled {
    pub enabled: bool,
    pub listening: bool,
    pub address: String,
    pub error: Option<String>,
    /// RFC 3339; `None` if nothing ever arrived.
    pub last_datagram_at: Option<String>,
    pub datagrams: Datagrams,
    pub connections: Connections,
    pub bindings: Bindings,
    pub names: Names,
    pub errors: Errors,
    pub named_share_15m: Option<f64>,
}

#[derive(Serialize)]
pub struct Disabled {
    pub enabled: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum CaptureStatus {
    Enabled(Box<Enabled>),
    Disabled(Disabled),
}

fn rfc3339(ms: i64) -> Option<String> {
    let t = time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)).ok()?;
    t.format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn enabled(s: CaptureStats) -> Enabled {
    let c = s.counters;
    let w = s.window;
    Enabled {
        enabled: true,
        listening: s.listening,
        address: s.address.to_string(),
        error: s.error,
        last_datagram_at: s.last_datagram_ms.and_then(rfc3339),
        datagrams: Datagrams {
            received: c.datagrams_received,
            rejected_source: c.rejected_source,
            dropped_backpressure: c.dropped_backpressure,
            unsupported: c.unsupported(),
            malformed: c.malformed(),
            max_len_15m: w.max_datagram_len,
        },
        connections: Connections {
            tracked: s.conns_tracked,
            evicted: c.conn_evicted_lru + c.conn_evicted_ttl + c.conn_evicted_bytes,
            reassembly_bytes: s.reassembly_bytes,
        },
        bindings: Bindings {
            tracked: s.bindings_tracked,
            evicted: c.bindings_evicted,
        },
        names: Names {
            tls: c.tls_named,
            quic: c.quic_named,
            http: c.http_named,
            dns: c.dns_bindings,
            ech: c.tls_ech,
        },
        errors: Errors {
            tls_incomplete: c.tls_incomplete,
            tls_parse: c.tls_parse_error,
            quic_unsupported_version: c.quic_unsupported_version,
            quic_decrypt: c.quic_decrypt_error,
            quic_incomplete: c.quic_incomplete,
            dns_parse: c.dns_parse_error,
            name_invalid: c.name_invalid,
        },
        named_share_15m: (w.lookups > 0).then(|| w.lookup_hits as f64 / w.lookups as f64),
    }
}

pub fn status(h: Option<&CaptureHandle>, now_ms: i64) -> CaptureStatus {
    match h {
        Some(h) => CaptureStatus::Enabled(Box::new(enabled(h.stats(now_ms)))),
        None => CaptureStatus::Disabled(Disabled { enabled: false }),
    }
}
