//! Rule evaluation: candidates from indexes, then full match in priority order.

use std::net::IpAddr;

use super::compile::{CompiledRule, RuleSet};
use crate::category::{Categorizer, OTHER_ID};
use crate::flow::FlowRecord;

impl RuleSet {
    /// First fully matching rule in priority order, if any.
    pub fn find(&self, f: &FlowRecord) -> Option<&CompiledRule> {
        let mut cands = self.candidates(f);
        cands.sort_unstable();
        cands.dedup();
        cands
            .into_iter()
            .map(|i| &self.rules[i])
            .find(|r| r.matches(f))
    }

    fn candidates(&self, f: &FlowRecord) -> Vec<usize> {
        let idx = &self.idx;
        let mut out: Vec<usize> = idx.always.clone();
        match f.remote_ip {
            IpAddr::V4(a) => idx
                .v4
                .cover(&(u32::from(a), 32))
                .for_each(|(_, v)| out.extend_from_slice(v)),
            IpAddr::V6(a) => idx
                .v6
                .cover(&(u128::from(a), 128))
                .for_each(|(_, v)| out.extend_from_slice(v)),
        }
        if let Some(set) = &idx.hostnames {
            let aliases = f.remote_aliases.as_deref().unwrap_or_default();
            for host in f.remote_host.iter().chain(aliases) {
                let host = host.to_ascii_lowercase();
                out.extend(
                    set.matches(&host)
                        .into_iter()
                        .map(|g| idx.hostname_rules[g]),
                );
            }
        }
        for (name, map) in [(&f.app, &idx.app), (&f.app_category, &idx.app_category)] {
            if let Some(v) = name
                .as_deref()
                .and_then(|n| map.get(&n.to_ascii_lowercase()))
            {
                out.extend_from_slice(v);
            }
        }
        if let Some(v) = f.asn.and_then(|a| idx.asn.get(&a)) {
            out.extend_from_slice(v);
        }
        if let Some(v) = f.country.as_deref().and_then(|c| idx.country.get(c)) {
            out.extend_from_slice(v);
        }
        if let Some(v) = idx.port.get(&f.remote_port) {
            out.extend_from_slice(v);
        }
        out.extend(
            idx.port_ranges
                .iter()
                .filter(|(p, _)| p.contains(f.remote_port))
                .map(|x| x.1),
        );
        out
    }
}

impl Categorizer for RuleSet {
    fn categorize(&self, flow: &FlowRecord) -> (u16, Option<u32>) {
        self.find(flow)
            .map_or((OTHER_ID, None), |r| (r.category_id, Some(r.id)))
    }
}

/// Unset list matches everything; a set list needs a (case-insensitive) hit.
fn named(list: &Option<Vec<String>>, value: Option<&str>) -> bool {
    list.as_ref()
        .is_none_or(|l| value.is_some_and(|v| l.iter().any(|x| x.eq_ignore_ascii_case(v))))
}

fn any<T>(opt: &Option<Vec<T>>, pred: impl Fn(&T) -> bool) -> bool {
    opt.as_ref().is_none_or(|v| v.iter().any(pred))
}

impl CompiledRule {
    /// AND across set fields; OR within each list.
    pub fn matches(&self, f: &FlowRecord) -> bool {
        any(&self.proto, |p| *p == f.proto)
            && any(&self.remote_port, |p| p.contains(f.remote_port))
            && any(&self.local_port, |p| p.contains(f.local_port))
            && any(&self.remote_cidr, |c| c.contains(&f.remote_ip))
            && any(&self.local_host, |c| c.contains(&f.local_ip))
            && any(&self.asn, |a| Some(*a) == f.asn)
            && any(&self.country, |c| Some(c.as_str()) == f.country.as_deref())
            && any(&self.interface, |i| *i == f.in_if || *i == f.out_if)
            && any(&self.direction, |d| *d == f.direction)
            && named(&self.app, f.app.as_deref())
            && named(&self.app_category, f.app_category.as_deref())
            && self.hostname_matches(f)
    }

    /// How sure a categorization by this rule is for this flow (FR-008).
    pub fn confidence_for(&self, f: &FlowRecord) -> crate::rules::Confidence {
        use crate::rules::Confidence;
        if self.by_hostname || (self.by_app && f.app_confident) {
            Confidence::Detected
        } else if self.by_app {
            Confidence::Inferred
        } else {
            self.base_confidence
        }
    }

    fn hostname_matches(&self, f: &FlowRecord) -> bool {
        let Some(set) = &self.hostname else {
            return true;
        };
        let is = |h: &str| set.is_match(h.to_ascii_lowercase().as_str());
        f.remote_host.as_deref().is_some_and(is)
            || f.remote_aliases
                .as_deref()
                .is_some_and(|a| a.iter().any(|h| is(h)))
    }
}

/// Naive linear evaluation (reference implementation for property tests).
pub fn categorize_linear(set: &RuleSet, f: &FlowRecord) -> (u16, Option<u32>) {
    set.rules
        .iter()
        .find(|r| r.matches(f))
        .map_or((OTHER_ID, None), |r| (r.category_id, Some(r.id)))
}
