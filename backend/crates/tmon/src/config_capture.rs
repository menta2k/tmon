//! `[capture]` section (feature 002, contracts/capture-input.md).

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::Duration;
use tmon_capture::{CaptureSettings, Limits};

use crate::config::ConfigError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureConfig {
    pub enabled: bool,
    pub listen: String,
    pub allowed_sources: Vec<String>,
    pub socket_recv_buffer_bytes: usize,
    pub queue_capacity: usize,
    pub conn_ttl_secs: u64,
    pub max_connections: usize,
    pub max_reassembly_bytes_per_conn: usize,
    pub max_reassembly_bytes_total: usize,
    pub max_device_bindings: usize,
    pub reassembly_timeout_secs: u64,
    pub silent_warning_secs: u64,
}

const TTL_RANGE_SECS: std::ops::RangeInclusive<u64> = 60..=3600;
/// Upper bound for table and queue sizes (entries).
const MAX_TABLE_ENTRIES: usize = 5_000_000;
/// Upper bound for buffered reassembly bytes (1 GiB).
const MAX_REASSEMBLY_TOTAL: usize = 1 << 30;

impl Default for CaptureConfig {
    fn default() -> Self {
        let d = CaptureSettings::default();
        let l = Limits::default();
        Self {
            enabled: false,
            listen: d.listen.to_string(),
            allowed_sources: Vec::new(),
            socket_recv_buffer_bytes: d.socket_recv_buffer_bytes,
            queue_capacity: d.queue_capacity,
            conn_ttl_secs: (l.conn_ttl_ms / 1000) as u64,
            max_connections: l.max_connections,
            max_reassembly_bytes_per_conn: l.max_reassembly_bytes_per_conn,
            max_reassembly_bytes_total: l.max_reassembly_bytes_total,
            max_device_bindings: l.max_device_bindings,
            reassembly_timeout_secs: (l.reassembly_timeout_ms / 1000) as u64,
            silent_warning_secs: d.silent_after.as_secs(),
        }
    }
}

fn bad(key: &str, why: impl std::fmt::Display) -> ConfigError {
    ConfigError::Invalid(format!("capture.{key}: {why}"))
}

impl CaptureConfig {
    /// `None` when disabled; otherwise validated settings (fail fast).
    pub fn validate(&self) -> Result<Option<CaptureSettings>, ConfigError> {
        if !self.enabled {
            return Ok(None);
        }
        let listen = self
            .listen
            .parse()
            .map_err(|_| bad("listen", format!("invalid address `{}`", self.listen)))?;
        let allowed_sources = self
            .allowed_sources
            .iter()
            .map(|s| {
                s.parse::<IpAddr>()
                    .map_err(|_| bad("allowed_sources", format!("invalid IP address `{s}`")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.check_limits()?;
        Ok(Some(CaptureSettings {
            listen,
            allowed_sources,
            socket_recv_buffer_bytes: self.socket_recv_buffer_bytes,
            queue_capacity: self.queue_capacity,
            silent_after: Duration::from_secs(self.silent_warning_secs),
            limits: Limits {
                conn_ttl_ms: secs_ms(self.conn_ttl_secs),
                max_connections: self.max_connections,
                max_reassembly_bytes_per_conn: self.max_reassembly_bytes_per_conn,
                max_reassembly_bytes_total: self.max_reassembly_bytes_total,
                max_device_bindings: self.max_device_bindings,
                reassembly_timeout_ms: secs_ms(self.reassembly_timeout_secs),
            },
        }))
    }

    fn check_limits(&self) -> Result<(), ConfigError> {
        let positive = [
            (
                "socket_recv_buffer_bytes",
                self.socket_recv_buffer_bytes as u64,
            ),
            ("queue_capacity", self.queue_capacity as u64),
            ("max_connections", self.max_connections as u64),
            (
                "max_reassembly_bytes_per_conn",
                self.max_reassembly_bytes_per_conn as u64,
            ),
            (
                "max_reassembly_bytes_total",
                self.max_reassembly_bytes_total as u64,
            ),
            ("max_device_bindings", self.max_device_bindings as u64),
            ("reassembly_timeout_secs", self.reassembly_timeout_secs),
            ("silent_warning_secs", self.silent_warning_secs),
        ];
        if let Some((key, _)) = positive.iter().find(|(_, v)| *v == 0) {
            return Err(bad(key, "must be > 0"));
        }
        // LRU tables preallocate: refuse sizes that could exhaust memory at startup.
        let bounded = [
            ("max_connections", self.max_connections, MAX_TABLE_ENTRIES),
            (
                "max_device_bindings",
                self.max_device_bindings,
                MAX_TABLE_ENTRIES,
            ),
            ("queue_capacity", self.queue_capacity, MAX_TABLE_ENTRIES),
            (
                "max_reassembly_bytes_total",
                self.max_reassembly_bytes_total,
                MAX_REASSEMBLY_TOTAL,
            ),
        ];
        if let Some((key, _, max)) = bounded.iter().find(|(_, v, max)| v > max) {
            return Err(bad(key, format!("must be <= {max}")));
        }
        if self.max_reassembly_bytes_per_conn > self.max_reassembly_bytes_total {
            return Err(bad(
                "max_reassembly_bytes_per_conn",
                "must be <= max_reassembly_bytes_total",
            ));
        }
        if !TTL_RANGE_SECS.contains(&self.conn_ttl_secs) {
            return Err(bad("conn_ttl_secs", "must be between 60 and 3600"));
        }
        Ok(())
    }
}

fn secs_ms(s: u64) -> i64 {
    i64::try_from(s.saturating_mul(1000)).unwrap_or(i64::MAX)
}
