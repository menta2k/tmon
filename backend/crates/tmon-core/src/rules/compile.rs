//! Compile an ordered rule list into an immutable, indexed `RuleSet`.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use prefix_trie::PrefixMap;
use std::collections::HashMap;
use std::net::IpAddr;

use super::model::{HostSpec, PortSpec, Rule};
use crate::cidr::Cidr;
use crate::flow::Direction;

/// A rule prepared for fast evaluation.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub id: u32,
    pub category_id: u16,
    pub priority: i64,
    pub(crate) proto: Option<Vec<u8>>,
    pub(crate) remote_port: Option<Vec<PortSpec>>,
    pub(crate) local_port: Option<Vec<PortSpec>>,
    pub(crate) remote_cidr: Option<Vec<Cidr>>,
    /// MACs are resolved to host CIDRs at compile time; unresolved MACs never match.
    pub(crate) local_host: Option<Vec<Cidr>>,
    pub(crate) asn: Option<Vec<u32>>,
    pub(crate) country: Option<Vec<String>>,
    pub(crate) interface: Option<Vec<u32>>,
    pub(crate) hostname: Option<GlobSet>,
    pub(crate) direction: Option<Vec<Direction>>,
    /// Lowercased application names.
    pub(crate) app: Option<Vec<String>>,
    /// Lowercased classifier categories.
    pub(crate) app_category: Option<Vec<String>>,
    /// Matched on an application field (confidence depends on the flow).
    pub(crate) by_app: bool,
    /// Matched on a host name (always "detected").
    pub(crate) by_hostname: bool,
    /// Rule-level confidence (used when not matched on an app or host name).
    pub(crate) base_confidence: crate::rules::Confidence,
}

/// Candidate-selection indexes. Each rule is indexed under exactly one
/// field (all its values); rules without an indexable field go to `always`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Indexes {
    pub v4: PrefixMap<(u32, u8), Vec<usize>>,
    pub v6: PrefixMap<(u128, u8), Vec<usize>>,
    pub hostnames: Option<GlobSet>,
    pub hostname_rules: Vec<usize>,
    pub asn: HashMap<u32, Vec<usize>>,
    pub country: HashMap<String, Vec<usize>>,
    /// Lowercased application name → rules.
    pub app: HashMap<String, Vec<usize>>,
    /// Lowercased classifier category → rules.
    pub app_category: HashMap<String, Vec<usize>>,
    pub port: HashMap<u16, Vec<usize>>,
    pub port_ranges: Vec<(PortSpec, usize)>,
    pub always: Vec<usize>,
}

/// Immutable compiled rule set; rules sorted by priority (desc).
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    pub(crate) rules: Vec<CompiledRule>,
    pub(crate) idx: Indexes,
}

impl RuleSet {
    /// Compile enabled rules. `mac_to_ips` resolves MAC host specs.
    pub fn compile(rules: &[Rule], mac_to_ips: &HashMap<String, Vec<IpAddr>>) -> Self {
        let mut enabled: Vec<&Rule> = rules.iter().filter(|r| r.enabled).collect();
        enabled.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
        let compiled: Vec<CompiledRule> = enabled
            .iter()
            .map(|r| compile_rule(r, mac_to_ips))
            .collect();
        let idx = build_indexes(&enabled);
        Self {
            rules: compiled,
            idx,
        }
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

fn compile_rule(r: &Rule, mac_to_ips: &HashMap<String, Vec<IpAddr>>) -> CompiledRule {
    let m = &r.match_;
    let local_host = m.local_host.as_ref().map(|hosts| {
        hosts
            .iter()
            .flat_map(|h| match h {
                HostSpec::Net(c) => vec![*c],
                HostSpec::Mac(mac) => mac_to_ips
                    .get(mac)
                    .map(|ips| ips.iter().map(|ip| Cidr::host(*ip)).collect())
                    .unwrap_or_default(),
            })
            .collect()
    });
    CompiledRule {
        id: r.id,
        category_id: r.category_id,
        priority: r.priority,
        proto: m.proto.clone(),
        remote_port: m.remote_port.clone(),
        local_port: m.local_port.clone(),
        remote_cidr: m.remote_cidr.clone(),
        local_host,
        asn: m.asn.clone(),
        country: m.country.clone(),
        interface: m.interface.clone(),
        hostname: m.hostname.as_ref().and_then(|g| build_globset(g)),
        direction: m.direction.clone(),
        app: lower(&m.app),
        app_category: lower(&m.app_category),
        by_app: m.app.is_some() || m.app_category.is_some(),
        by_hostname: m.hostname.is_some(),
        base_confidence: m.confidence(),
    }
}

fn lower(v: &Option<Vec<String>>) -> Option<Vec<String>> {
    v.as_ref()
        .map(|l| l.iter().map(|s| s.to_ascii_lowercase()).collect())
}

fn build_globset(patterns: &[String]) -> Option<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let glob = GlobBuilder::new(p)
            .case_insensitive(true)
            .literal_separator(false)
            .build();
        b.add(glob.ok()?);
    }
    b.build().ok()
}

fn build_indexes(rules: &[&Rule]) -> Indexes {
    let mut idx = Indexes::default();
    let mut host_globs: Vec<String> = Vec::new();
    for (i, r) in rules.iter().enumerate() {
        let m = &r.match_;
        if let Some(cidrs) = &m.remote_cidr {
            for c in cidrs {
                if let Some(k) = c.v4_key() {
                    push(&mut idx.v4, k, i);
                } else if let Some(k) = c.v6_key() {
                    push(&mut idx.v6, k, i);
                }
            }
        } else if let Some(hosts) = &m.hostname {
            for h in hosts {
                host_globs.push(h.clone());
                idx.hostname_rules.push(i);
            }
        } else if let Some(apps) = &m.app {
            apps.iter()
                .for_each(|a| idx.app.entry(a.to_ascii_lowercase()).or_default().push(i));
        } else if let Some(cats) = &m.app_category {
            cats.iter().for_each(|c| {
                idx.app_category
                    .entry(c.to_ascii_lowercase())
                    .or_default()
                    .push(i)
            });
        } else if let Some(asns) = &m.asn {
            asns.iter()
                .for_each(|a| idx.asn.entry(*a).or_default().push(i));
        } else if let Some(cc) = &m.country {
            cc.iter()
                .for_each(|c| idx.country.entry(c.clone()).or_default().push(i));
        } else if let Some(ports) = &m.remote_port {
            for p in ports {
                if p.is_single() {
                    idx.port.entry(p.lo).or_default().push(i);
                } else {
                    idx.port_ranges.push((*p, i));
                }
            }
        } else {
            idx.always.push(i);
        }
    }
    idx.hostnames = build_globset(&host_globs);
    idx
}

fn push<K: prefix_trie::Prefix>(map: &mut PrefixMap<K, Vec<usize>>, key: K, i: usize) {
    if let Some(v) = map.get_mut(&key) {
        v.push(i);
    } else {
        map.insert(key, vec![i]);
    }
}
