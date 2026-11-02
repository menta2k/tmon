//! Normalized flow records (data-model.md §1).
//!
//! `RawFlow` is what decoders produce (src/dst form). `FlowRecord` is the
//! classified, enriched, immutable record that gets stored. Enrichment uses
//! `with_*` copy-update methods; there are no setters.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::Arc;

use crate::app::AppInfo;
use crate::category::OTHER_ID;
use crate::naming::NameSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Down,
    Up,
    Internal,
    Transit,
}

impl Direction {
    pub const ALL: [Direction; 4] = [
        Direction::Down,
        Direction::Up,
        Direction::Internal,
        Direction::Transit,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::Down => "down",
            Direction::Up => "up",
            Direction::Internal => "internal",
            Direction::Transit => "transit",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.as_str() == s)
    }
}

/// Decoder output before direction classification and enrichment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFlow {
    /// Flow end, epoch milliseconds (UTC).
    pub ts_ms: i64,
    /// Flow start, epoch milliseconds (UTC).
    pub ts_start_ms: i64,
    pub exporter_id: u16,
    pub in_if: u32,
    pub out_if: u32,
    pub src_ip: IpAddr,
    pub dst_ip: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub proto: u8,
    pub tcp_flags: u8,
    /// Already multiplied by the sampling rate.
    pub bytes: u64,
    /// Already multiplied by the sampling rate.
    pub packets: u64,
    pub sampled: bool,
    /// Post-NAT source (IPFIX 225/281, 227), only when it differs from `src_ip`/`src_port`.
    pub nat_ip: Option<IpAddr>,
    pub nat_port: Option<u16>,
    /// Post-NAT destination (IPFIX 226/282, 228), only when it differs from `dst_ip`/`dst_port`.
    /// RouterOS exporting on the WAN puts the LAN host of a download here.
    pub nat_dst_ip: Option<IpAddr>,
    pub nat_dst_port: Option<u16>,
    /// Exporter-reported flowDirection (IPFIX IE 61): 0 = ingress, 1 = egress.
    pub flow_direction: Option<u8>,
}

/// Classified + enriched flow (immutable once built).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlowRecord {
    pub ts_ms: i64,
    pub ts_start_ms: i64,
    pub exporter_id: u16,
    pub in_if: u32,
    pub out_if: u32,
    pub ip_ver: u8,
    pub local_ip: IpAddr,
    pub remote_ip: IpAddr,
    pub local_port: u16,
    pub remote_port: u16,
    pub proto: u8,
    pub tcp_flags: u8,
    pub bytes: u64,
    pub packets: u64,
    pub sampled: bool,
    pub direction: Direction,
    pub nat_ip: Option<IpAddr>,
    pub nat_port: Option<u16>,
    /// Never null: falls back to "Other" (id 0).
    pub category_id: u16,
    pub rule_id: Option<u32>,
    pub country: Option<Arc<str>>,
    pub asn: Option<u32>,
    pub as_name: Option<Arc<str>>,
    pub remote_host: Option<Arc<str>>,
    /// Where `remote_host` came from (persisted as `remote_host_source`).
    pub remote_host_source: Option<NameSource>,
    /// CNAME chain for DNS-derived names; used only for rule matching (R11a).
    #[serde(skip)]
    pub remote_aliases: Option<Arc<[Arc<str>]>>,
    /// Application from the classifier (feature 003), e.g. `BitTorrent`.
    pub app: Option<Arc<str>>,
    /// The application was detected with confidence (false when `app` is None).
    pub app_confident: bool,
    /// Classifier category; used only for rule matching at ingest (not stored).
    #[serde(skip)]
    pub app_category: Option<Arc<str>>,
}

impl FlowRecord {
    /// Build from a raw flow and its direction classification.
    /// `local_is_src` tells which side of the raw flow is "local".
    pub fn from_raw(raw: &RawFlow, direction: Direction, local_is_src: bool) -> Self {
        let (local_ip, remote_ip, local_port, remote_port) = if local_is_src {
            (raw.src_ip, raw.dst_ip, raw.src_port, raw.dst_port)
        } else {
            (raw.dst_ip, raw.src_ip, raw.dst_port, raw.src_port)
        };
        let ts_start_ms = raw.ts_start_ms.min(raw.ts_ms);
        Self {
            ts_ms: raw.ts_ms,
            ts_start_ms,
            exporter_id: raw.exporter_id,
            in_if: raw.in_if,
            out_if: raw.out_if,
            ip_ver: if raw.src_ip.is_ipv4() { 4 } else { 6 },
            local_ip,
            remote_ip,
            local_port,
            remote_port,
            proto: raw.proto,
            tcp_flags: raw.tcp_flags,
            bytes: raw.bytes,
            packets: raw.packets,
            sampled: raw.sampled,
            direction,
            nat_ip: raw.nat_ip,
            nat_port: raw.nat_port,
            category_id: OTHER_ID,
            rule_id: None,
            country: None,
            asn: None,
            as_name: None,
            remote_host: None,
            remote_host_source: None,
            remote_aliases: None,
            app: None,
            app_confident: false,
            app_category: None,
        }
    }

    pub fn with_category(&self, category_id: u16, rule_id: Option<u32>) -> Self {
        Self {
            category_id,
            rule_id,
            ..self.clone()
        }
    }

    pub fn with_geo(
        &self,
        country: Option<Arc<str>>,
        asn: Option<u32>,
        as_name: Option<Arc<str>>,
    ) -> Self {
        Self {
            country,
            asn,
            as_name,
            ..self.clone()
        }
    }

    pub fn with_remote_host(&self, remote_host: Option<Arc<str>>) -> Self {
        Self {
            remote_host,
            ..self.clone()
        }
    }

    /// Set the remote name together with its source.
    pub fn with_remote_name(&self, host: Arc<str>, source: NameSource) -> Self {
        Self {
            remote_host: Some(host),
            remote_host_source: Some(source),
            ..self.clone()
        }
    }

    /// Attach a classification (FR-004).
    pub fn with_app(&self, info: &AppInfo) -> Self {
        Self {
            app: Some(Arc::clone(&info.app)),
            app_confident: info.confident,
            app_category: (!info.category.is_empty()).then(|| Arc::clone(&info.category)),
            ..self.clone()
        }
    }

    /// Set the stored application fields (used when reading rows back).
    pub fn with_stored_app(&self, app: Option<Arc<str>>, confident: Option<bool>) -> Self {
        Self {
            app_confident: app.is_some() && confident.unwrap_or(false),
            app,
            ..self.clone()
        }
    }

    /// Set the alias chain (empty → `None`).
    pub fn with_aliases(&self, aliases: Arc<[Arc<str>]>) -> Self {
        Self {
            remote_aliases: (!aliases.is_empty()).then_some(aliases),
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> RawFlow {
        RawFlow {
            ts_ms: 2_000,
            ts_start_ms: 3_000,
            exporter_id: 1,
            in_if: 1,
            out_if: 2,
            src_ip: "192.168.1.10".parse().unwrap(),
            dst_ip: "8.8.8.8".parse().unwrap(),
            src_port: 50000,
            dst_port: 53,
            proto: 17,
            tcp_flags: 0,
            bytes: 100,
            packets: 1,
            sampled: false,
            nat_ip: None,
            nat_port: None,
            nat_dst_ip: None,
            nat_dst_port: None,
            flow_direction: None,
        }
    }

    #[test]
    fn from_raw_swaps_sides_and_clamps_start() {
        let f = FlowRecord::from_raw(&raw(), Direction::Down, false);
        assert_eq!(f.local_ip.to_string(), "8.8.8.8");
        assert_eq!(f.remote_port, 50000);
        assert_eq!(f.ts_start_ms, 2_000);
        assert_eq!(f.category_id, OTHER_ID);
        assert_eq!(f.ip_ver, 4);
    }

    #[test]
    fn with_methods_copy() {
        let f = FlowRecord::from_raw(&raw(), Direction::Up, true);
        let g = f
            .with_category(3, Some(9))
            .with_geo(Some("US".into()), Some(15169), None);
        assert_eq!(f.category_id, OTHER_ID);
        assert_eq!(g.category_id, 3);
        assert_eq!(g.asn, Some(15169));
        assert_eq!(
            g.with_remote_host(Some("x".into())).remote_host.as_deref(),
            Some("x")
        );
        assert_eq!(Direction::parse("transit"), Some(Direction::Transit));
        assert_eq!(Direction::parse("x"), None);
    }
}
