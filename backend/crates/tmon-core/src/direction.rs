//! Direction classification against the configured local networks (FR-007).

use std::net::IpAddr;

use crate::cidr::Cidr;
use crate::flow::{Direction, FlowRecord, RawFlow};

pub const DEFAULT_LOCAL_NETWORKS: [&str; 5] = [
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "100.64.0.0/10",
    "fc00::/7",
];

/// Immutable set of "inside" networks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNetworks {
    nets: Vec<Cidr>,
}

impl LocalNetworks {
    pub fn new(nets: Vec<Cidr>) -> Self {
        Self { nets }
    }

    pub fn defaults() -> Self {
        let nets = DEFAULT_LOCAL_NETWORKS
            .iter()
            .map(|s| s.parse().expect("default networks are valid"))
            .collect();
        Self { nets }
    }

    pub fn nets(&self) -> &[Cidr] {
        &self.nets
    }

    pub fn contains(&self, ip: &IpAddr) -> bool {
        self.nets.iter().any(|n| n.contains(ip))
    }

    /// Classify a raw flow:
    /// `src∈L ∧ dst∉L → up`, `src∉L ∧ dst∈L → down`, both → internal,
    /// neither → transit. Returns the direction and whether the source is
    /// the "local" side (true for up/internal/transit).
    pub fn classify(&self, raw: &RawFlow) -> (Direction, bool) {
        match (self.contains(&raw.src_ip), self.contains(&raw.dst_ip)) {
            (true, false) => (Direction::Up, true),
            (false, true) => (Direction::Down, false),
            (true, true) => (Direction::Internal, true),
            (false, false) => (Direction::Transit, true),
        }
    }

    /// Classify and build the `FlowRecord` in one step.
    pub fn to_record(&self, raw: &RawFlow) -> FlowRecord {
        let inside = self.un_nat(raw);
        let (direction, local_is_src) = self.classify(&inside);
        FlowRecord::from_raw(&inside, direction, local_is_src)
    }

    /// The flow as seen from the LAN, with `nat_ip`/`nat_port` holding the
    /// router's outside (WAN) endpoint.
    ///
    /// RouterOS exports on the WAN interface: uploads carry the LAN host as the
    /// pre-NAT source, but downloads are addressed to the WAN IP and carry the
    /// LAN host only in the post-NAT destination.
    fn un_nat(&self, raw: &RawFlow) -> RawFlow {
        let lan_dst = raw
            .nat_dst_ip
            .filter(|ip| *ip != raw.dst_ip && self.contains(ip));
        match lan_dst {
            Some(lan) => RawFlow {
                dst_ip: lan,
                dst_port: raw.nat_dst_port.unwrap_or(raw.dst_port),
                nat_ip: Some(raw.dst_ip),
                nat_port: Some(raw.dst_port),
                nat_dst_ip: None,
                nat_dst_port: None,
                ..raw.clone()
            },
            None => {
                // Exporters repeat the original address when nothing was translated.
                let nat_ip = raw.nat_ip.filter(|ip| *ip != raw.src_ip);
                RawFlow {
                    nat_ip,
                    nat_port: nat_ip.and(raw.nat_port),
                    nat_dst_ip: None,
                    nat_dst_port: None,
                    ..raw.clone()
                }
            }
        }
    }
}

impl Default for LocalNetworks {
    fn default() -> Self {
        Self::defaults()
    }
}
