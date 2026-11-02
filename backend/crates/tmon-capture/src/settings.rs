//! Validated capture settings (contracts/capture-input.md `[capture]`).

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// Memory and time bounds (FR-009, FR-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub conn_ttl_ms: i64,
    pub max_connections: usize,
    pub max_reassembly_bytes_per_conn: usize,
    pub max_reassembly_bytes_total: usize,
    pub max_device_bindings: usize,
    pub reassembly_timeout_ms: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            conn_ttl_ms: 600_000,
            max_connections: 100_000,
            max_reassembly_bytes_per_conn: 16 * 1024,
            max_reassembly_bytes_total: 64 * 1024 * 1024,
            max_device_bindings: 200_000,
            reassembly_timeout_ms: 10_000,
        }
    }
}

/// Everything the capture listener needs, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSettings {
    pub listen: SocketAddr,
    /// Empty = accept every sender.
    pub allowed_sources: Vec<IpAddr>,
    pub socket_recv_buffer_bytes: usize,
    pub queue_capacity: usize,
    pub silent_after: Duration,
    pub limits: Limits,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], 37008)),
            allowed_sources: Vec::new(),
            socket_recv_buffer_bytes: 4 * 1024 * 1024,
            queue_capacity: 16_384,
            silent_after: Duration::from_secs(300),
            limits: Limits::default(),
        }
    }
}
