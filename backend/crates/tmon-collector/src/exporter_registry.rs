//! Exporter registry (data-model.md §3): stable ids, settings, state.

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicI64, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::counters::{CountersSnapshot, ExporterCounters};

pub const STALL_AFTER_MS: i64 = 5 * 60 * 1000;
/// Hard cap on distinct (address, domain) exporters; protects against spoofed/varied headers.
pub const MAX_EXPORTERS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowProtocol {
    Netflow5,
    Netflow9,
    Ipfix,
    Sflow5,
}

impl FlowProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlowProtocol::Netflow5 => "netflow5",
            FlowProtocol::Netflow9 => "netflow9",
            FlowProtocol::Ipfix => "ipfix",
            FlowProtocol::Sflow5 => "sflow5",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Netflow5, Self::Netflow9, Self::Ipfix, Self::Sflow5]
            .into_iter()
            .find(|p| p.as_str() == s)
    }
    fn to_u8(self) -> u8 {
        self as u8
    }
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Netflow5,
            1 => Self::Netflow9,
            2 => Self::Ipfix,
            _ => Self::Sflow5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CountDirection {
    Ingress,
    Egress,
    #[default]
    Both,
}

impl CountDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ingress => "ingress",
            Self::Egress => "egress",
            Self::Both => "both",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Ingress, Self::Egress, Self::Both]
            .into_iter()
            .find(|d| d.as_str() == s)
    }
}

/// User-editable settings of an exporter (immutable value, swapped atomically).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ExporterSettings {
    pub name: Option<String>,
    /// ≥ 1; overrides the advertised sampling rate.
    pub sampling_override: Option<u32>,
    pub count_direction: CountDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExporterState {
    Active,
    Stalled,
}

/// Persistable / reportable view of an exporter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExporterInfo {
    pub id: u16,
    pub address: IpAddr,
    pub domain_id: u32,
    pub protocol: FlowProtocol,
    pub name: Option<String>,
    pub sampling_override: Option<u32>,
    pub count_direction: CountDirection,
    pub sampling_rate: u32,
    pub first_seen: i64,
    pub last_seen: i64,
    pub state: ExporterState,
    pub counters: CountersSnapshot,
}

#[derive(Debug)]
pub struct Exporter {
    pub id: u16,
    pub address: IpAddr,
    pub domain_id: u32,
    pub first_seen: i64,
    protocol: AtomicU8,
    last_seen: AtomicI64,
    advertised_sampling: AtomicU32,
    settings: ArcSwap<ExporterSettings>,
    pub counters: ExporterCounters,
}

impl Exporter {
    pub fn settings(&self) -> Arc<ExporterSettings> {
        self.settings.load_full()
    }

    /// Effective sampling rate (override wins, then advertised, else 1).
    pub fn sampling_rate(&self) -> u32 {
        self.settings
            .load()
            .sampling_override
            .unwrap_or_else(|| self.advertised_sampling.load(Ordering::Relaxed))
            .max(1)
    }

    pub fn set_advertised_sampling(&self, rate: u32) {
        self.advertised_sampling.store(rate, Ordering::Relaxed);
    }

    pub fn touch(&self, protocol: FlowProtocol, now_ms: i64) {
        self.protocol.store(protocol.to_u8(), Ordering::Relaxed);
        self.last_seen.fetch_max(now_ms, Ordering::Relaxed);
    }

    pub fn protocol(&self) -> FlowProtocol {
        FlowProtocol::from_u8(self.protocol.load(Ordering::Relaxed))
    }

    pub fn last_seen(&self) -> i64 {
        self.last_seen.load(Ordering::Relaxed)
    }

    pub fn state(&self, now_ms: i64, stall_after_ms: i64) -> ExporterState {
        if now_ms - self.last_seen() > stall_after_ms {
            ExporterState::Stalled
        } else {
            ExporterState::Active
        }
    }

    pub fn info(&self, now_ms: i64, stall_after_ms: i64) -> ExporterInfo {
        let s = self.settings();
        ExporterInfo {
            id: self.id,
            address: self.address,
            domain_id: self.domain_id,
            protocol: self.protocol(),
            name: s.name.clone(),
            sampling_override: s.sampling_override,
            count_direction: s.count_direction,
            sampling_rate: self.sampling_rate(),
            first_seen: self.first_seen,
            last_seen: self.last_seen(),
            state: self.state(now_ms, stall_after_ms),
            counters: self.counters.snapshot(),
        }
    }
}

/// Previously persisted exporter used to seed the registry at startup.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownExporter {
    pub id: u16,
    pub address: IpAddr,
    pub domain_id: u32,
    pub protocol: FlowProtocol,
    pub settings: ExporterSettings,
    pub first_seen: i64,
    pub last_seen: i64,
}

type Key = (IpAddr, u32);

/// Copy-on-write registry keyed by `(address, domain_id)`.
#[derive(Debug, Default)]
pub struct ExporterRegistry {
    map: ArcSwap<HashMap<Key, Arc<Exporter>>>,
    create_lock: Mutex<()>,
}

impl ExporterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_known(known: Vec<KnownExporter>) -> Self {
        let map: HashMap<Key, Arc<Exporter>> = known
            .into_iter()
            .map(|k| {
                let e = new_exporter(k.id, k.address, k.domain_id, k.protocol, k.first_seen);
                e.last_seen.store(k.last_seen, Ordering::Relaxed);
                e.settings.store(Arc::new(k.settings));
                ((k.address, k.domain_id), Arc::new(e))
            })
            .collect();
        Self {
            map: ArcSwap::from_pointee(map),
            create_lock: Mutex::new(()),
        }
    }

    /// Find or register an exporter. Returns `(exporter, is_new)`, or `None` once
    /// [`MAX_EXPORTERS`] distinct exporters exist and this one is unknown.
    pub fn get_or_create(
        &self,
        address: IpAddr,
        domain_id: u32,
        protocol: FlowProtocol,
        now_ms: i64,
    ) -> Option<(Arc<Exporter>, bool)> {
        if let Some(e) = self.map.load().get(&(address, domain_id)) {
            return Some((Arc::clone(e), false));
        }
        let _guard = self.create_lock.lock().unwrap_or_else(|p| p.into_inner());
        let current = self.map.load_full();
        if let Some(e) = current.get(&(address, domain_id)) {
            return Some((Arc::clone(e), false));
        }
        if current.len() >= MAX_EXPORTERS {
            return None;
        }
        let next_id = current
            .values()
            .map(|e| e.id)
            .max()
            .map_or(1, |m| m.saturating_add(1));
        let e = Arc::new(new_exporter(next_id, address, domain_id, protocol, now_ms));
        let mut next = (*current).clone();
        next.insert((address, domain_id), Arc::clone(&e));
        self.map.store(Arc::new(next));
        Some((e, true))
    }

    pub fn get_by_id(&self, id: u16) -> Option<Arc<Exporter>> {
        self.map.load().values().find(|e| e.id == id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<Exporter>> {
        let mut v: Vec<Arc<Exporter>> = self.map.load().values().cloned().collect();
        v.sort_by_key(|e| e.id);
        v
    }

    /// Replace an exporter's settings; returns false if unknown.
    pub fn update_settings(&self, id: u16, settings: ExporterSettings) -> bool {
        match self.get_by_id(id) {
            Some(e) => {
                e.settings.store(Arc::new(settings));
                true
            }
            None => false,
        }
    }

    pub fn snapshot(&self, now_ms: i64, stall_after_ms: i64) -> Vec<ExporterInfo> {
        self.all()
            .iter()
            .map(|e| e.info(now_ms, stall_after_ms))
            .collect()
    }
}

fn new_exporter(id: u16, address: IpAddr, domain_id: u32, p: FlowProtocol, now: i64) -> Exporter {
    Exporter {
        id,
        address,
        domain_id,
        first_seen: now,
        protocol: AtomicU8::new(p.to_u8()),
        last_seen: AtomicI64::new(now),
        advertised_sampling: AtomicU32::new(1),
        settings: ArcSwap::from_pointee(ExporterSettings::default()),
        counters: ExporterCounters::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_and_reuses() {
        let r = ExporterRegistry::new();
        let ip: IpAddr = "192.168.88.1".parse().unwrap();
        let (a, new_a) = r.get_or_create(ip, 0, FlowProtocol::Ipfix, 1000).unwrap();
        let (b, new_b) = r.get_or_create(ip, 0, FlowProtocol::Ipfix, 2000).unwrap();
        let (c, _) = r
            .get_or_create(ip, 7, FlowProtocol::Netflow9, 2000)
            .unwrap();
        assert!(new_a && !new_b);
        assert_eq!(a.id, b.id);
        assert_ne!(a.id, c.id);
        assert_eq!(r.all().len(), 2);
    }

    #[test]
    fn sampling_override_and_state() {
        let r = ExporterRegistry::new();
        let (e, _) = r
            .get_or_create("10.0.0.1".parse().unwrap(), 0, FlowProtocol::Sflow5, 0)
            .unwrap();
        e.set_advertised_sampling(512);
        assert_eq!(e.sampling_rate(), 512);
        let s = ExporterSettings {
            sampling_override: Some(4),
            ..Default::default()
        };
        assert!(r.update_settings(e.id, s));
        assert_eq!(e.sampling_rate(), 4);
        assert!(!r.update_settings(999, ExporterSettings::default()));
        e.touch(FlowProtocol::Sflow5, 1_000);
        assert_eq!(
            e.state(1_000 + STALL_AFTER_MS + 1, STALL_AFTER_MS),
            ExporterState::Stalled
        );
        assert_eq!(e.state(2_000, STALL_AFTER_MS), ExporterState::Active);
        let info = e.info(2_000, STALL_AFTER_MS);
        assert_eq!(info.protocol, FlowProtocol::Sflow5);
        assert_eq!(FlowProtocol::parse("ipfix"), Some(FlowProtocol::Ipfix));
        assert_eq!(
            CountDirection::parse("egress"),
            Some(CountDirection::Egress)
        );
    }

    #[test]
    fn seeds_known() {
        let k = KnownExporter {
            id: 5,
            address: "10.0.0.1".parse().unwrap(),
            domain_id: 0,
            protocol: FlowProtocol::Ipfix,
            settings: ExporterSettings {
                name: Some("gw".into()),
                ..Default::default()
            },
            first_seen: 1,
            last_seen: 2,
        };
        let r = ExporterRegistry::with_known(vec![k]);
        let (e, new) = r
            .get_or_create("10.0.0.1".parse().unwrap(), 0, FlowProtocol::Ipfix, 3)
            .unwrap();
        assert!(!new);
        assert_eq!(e.id, 5);
        let (n, _) = r
            .get_or_create("10.0.0.2".parse().unwrap(), 0, FlowProtocol::Ipfix, 3)
            .unwrap();
        assert_eq!(n.id, 6);
    }

    #[test]
    fn registry_is_capped() {
        let r = ExporterRegistry::new();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for d in 0..MAX_EXPORTERS as u32 {
            assert!(r.get_or_create(ip, d, FlowProtocol::Ipfix, 0).is_some());
        }
        assert!(
            r.get_or_create(ip, 9_999_999, FlowProtocol::Ipfix, 0)
                .is_none()
        );
        assert!(
            r.get_or_create(ip, 0, FlowProtocol::Ipfix, 0).is_some(),
            "known exporters still resolve"
        );
    }
}
