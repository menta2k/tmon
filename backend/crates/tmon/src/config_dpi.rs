//! `[dpi]` section (feature 003, contracts/config.md).

use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use tmon_dpi_client::DpiSettings;

use crate::config::{CaptureConfig, ConfigError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DpiConfig {
    pub enabled: bool,
    pub binary: String,
    pub max_flows: u64,
    pub max_packets_per_conn: u64,
    pub idle_timeout_secs: u64,
    pub memory_limit_mb: u64,
    pub queue_capacity: u64,
    pub queue_max_bytes: u64,
}

const MAX_FLOWS: RangeInclusive<u64> = 1_000..=1_000_000;
const MAX_PACKETS: RangeInclusive<u64> = 4..=256;
const IDLE_SECS: RangeInclusive<u64> = 60..=3_600;
const MEMORY_MB: RangeInclusive<u64> = 32..=2_048;
const QUEUE_CAPACITY: RangeInclusive<u64> = 64..=1_000_000;
const QUEUE_BYTES: RangeInclusive<u64> = (1 << 20)..=(256 << 20);

impl Default for DpiConfig {
    fn default() -> Self {
        let d = DpiSettings::default();
        Self {
            enabled: false,
            binary: d.binary,
            max_flows: u64::from(d.max_flows),
            max_packets_per_conn: u64::from(d.max_packets_per_conn),
            idle_timeout_secs: d.idle_timeout_ms / 1000,
            memory_limit_mb: u64::from(d.memory_limit_mb),
            queue_capacity: d.queue_capacity as u64,
            queue_max_bytes: d.queue_max_bytes as u64,
        }
    }
}

fn bad(key: &str, why: impl std::fmt::Display) -> ConfigError {
    ConfigError::Invalid(format!("dpi.{key}: {why}"))
}

fn in_range(key: &str, v: u64, r: &RangeInclusive<u64>) -> Result<u64, ConfigError> {
    if r.contains(&v) {
        Ok(v)
    } else {
        Err(bad(
            key,
            format!("must be between {} and {}", r.start(), r.end()),
        ))
    }
}

impl DpiConfig {
    /// `None` when disabled; otherwise validated settings (fail fast).
    /// A missing binary is not an error here: tmon starts without
    /// classification and warns (`dpi_not_found`).
    pub fn validate(&self, capture: &CaptureConfig) -> Result<Option<DpiSettings>, ConfigError> {
        if !self.enabled {
            return Ok(None);
        }
        if !capture.enabled {
            return Err(bad("enabled", "requires capture.enabled = true"));
        }
        let binary = self.binary.trim();
        if binary.is_empty() {
            return Err(bad("binary", "must not be empty"));
        }
        // Ranges keep every narrowing conversion below lossless.
        Ok(Some(DpiSettings {
            binary: binary.to_string(),
            max_flows: in_range("max_flows", self.max_flows, &MAX_FLOWS)? as u32,
            max_packets_per_conn: in_range(
                "max_packets_per_conn",
                self.max_packets_per_conn,
                &MAX_PACKETS,
            )? as u16,
            idle_timeout_ms: in_range("idle_timeout_secs", self.idle_timeout_secs, &IDLE_SECS)?
                * 1000,
            memory_limit_mb: in_range("memory_limit_mb", self.memory_limit_mb, &MEMORY_MB)? as u32,
            queue_capacity: in_range("queue_capacity", self.queue_capacity, &QUEUE_CAPACITY)?
                as usize,
            queue_max_bytes: in_range("queue_max_bytes", self.queue_max_bytes, &QUEUE_BYTES)?
                as usize,
            extra_args: Vec::new(),
        }))
    }
}
