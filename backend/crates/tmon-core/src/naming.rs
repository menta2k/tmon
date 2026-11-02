//! Connection naming seam (FR-013): who a flow's remote side is, and where
//! that name came from. Capture (`tmon-capture`) is one implementation of
//! [`ConnectionNamer`]; a future classifier can be another.

use serde::Serialize;
use std::net::IpAddr;
use std::sync::Arc;

use crate::flow::FlowRecord;

/// Longest accepted hostname (RFC 1035 presentation form, no trailing dot).
pub const MAX_HOST_LEN: usize = 253;
/// Longest accepted label.
pub const MAX_LABEL_LEN: usize = 63;
/// Most CNAME aliases kept per name.
pub const MAX_ALIASES: usize = 8;
/// Longest accepted ALPN protocol id.
pub const MAX_ALPN_LEN: usize = 32;

/// LAN-side connection identity (R8): equals the flow record after un-NAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnKey {
    pub lan_ip: IpAddr,
    pub lan_port: u16,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
    pub proto: u8,
}

impl ConnKey {
    pub fn from_record(f: &FlowRecord) -> Self {
        Self {
            lan_ip: f.local_ip,
            lan_port: f.local_port,
            remote_ip: f.remote_ip,
            remote_port: f.remote_port,
            proto: f.proto,
        }
    }
}

/// Where a remote host name came from (persisted as `remote_host_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum NameSource {
    Tls = 1,
    Quic = 2,
    Http = 3,
    Dns = 4,
    Router = 5,
    /// Host name reported by the application classifier (feature 003);
    /// lowest priority, used only when no other source named the flow.
    Dpi = 6,
}

impl NameSource {
    pub const ALL: [NameSource; 6] = [
        NameSource::Tls,
        NameSource::Quic,
        NameSource::Http,
        NameSource::Dns,
        NameSource::Router,
        NameSource::Dpi,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.code() == code)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NameSource::Tls => "tls",
            NameSource::Quic => "quic",
            NameSource::Http => "http",
            NameSource::Dns => "dns",
            NameSource::Router => "router",
            NameSource::Dpi => "dpi",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Why a name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("invalid hostname")]
    InvalidHost,
}

/// Lowercase ASCII, trailing `.` stripped, 1–253 bytes, labels 1–63 bytes of
/// `[a-z0-9_-]` (IDNA A-labels such as `xn--…` are plain ASCII and pass).
pub fn normalize_host(raw: &str) -> Option<String> {
    let trimmed = raw.strip_suffix('.').unwrap_or(raw);
    if trimmed.is_empty() || trimmed.len() > MAX_HOST_LEN || !trimmed.is_ascii() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    // All-numeric names (e.g. `192.168.1.1`) would read as addresses.
    let numeric = lower.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    (!numeric && lower.split('.').all(valid_label)).then_some(lower)
}

fn valid_label(l: &str) -> bool {
    (1..=MAX_LABEL_LEN).contains(&l.len())
        && l.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// ALPN id: at most 32 bytes, printable ASCII.
pub fn normalize_alpn(raw: &[u8]) -> Option<Arc<str>> {
    let ok =
        !raw.is_empty() && raw.len() <= MAX_ALPN_LEN && raw.iter().all(|b| b.is_ascii_graphic());
    if !ok {
        return None;
    }
    std::str::from_utf8(raw).ok().map(Arc::from)
}

/// A remote name for one connection or device binding. Immutable once built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnName {
    pub host: Arc<str>,
    pub aliases: Arc<[Arc<str>]>,
    pub alpn: Option<Arc<str>>,
    pub source: NameSource,
    /// The ClientHello carried `encrypted_client_hello`: `host` is the
    /// provider's public front name, not the destination (FR-008).
    pub ech_outer: bool,
}

impl ConnName {
    pub fn new(host: &str, source: NameSource) -> Result<Self, NameError> {
        let host = normalize_host(host).ok_or(NameError::InvalidHost)?;
        Ok(Self {
            host: Arc::from(host),
            aliases: Arc::from(Vec::new()),
            alpn: None,
            source,
            ech_outer: false,
        })
    }

    /// Valid aliases (at most [`MAX_ALIASES`], duplicates of `host` dropped).
    pub fn with_aliases<S: AsRef<str>>(self, aliases: &[S]) -> Self {
        let mut out: Vec<Arc<str>> = Vec::new();
        for a in aliases.iter().filter_map(|a| normalize_host(a.as_ref())) {
            if out.len() < MAX_ALIASES && *a != *self.host && !out.iter().any(|o| **o == *a) {
                out.push(Arc::from(a));
            }
        }
        Self {
            aliases: Arc::from(out),
            ..self
        }
    }

    pub fn with_alpn(self, alpn: Option<Arc<str>>) -> Self {
        Self { alpn, ..self }
    }

    pub fn with_ech_outer(self, ech_outer: bool) -> Self {
        Self { ech_outer, ..self }
    }
}

/// Result of one lookup: the exact connection entry and the device binding
/// for `(lan_ip, remote_ip)`. The pipeline applies the FR-010 order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameLookup {
    pub connection: Option<Arc<ConnName>>,
    pub device: Option<Arc<ConnName>>,
}

/// Replaceable naming step (FR-013).
pub trait ConnectionNamer: Send + Sync {
    fn lookup(&self, key: &ConnKey, now_ms: i64) -> NameLookup;
}
