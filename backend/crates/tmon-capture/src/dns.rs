//! DNS responses → per-device name bindings (R6, FR-006).

use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::rr::RData;
use std::collections::HashMap;
use std::net::IpAddr;
use tmon_core::naming::{MAX_ALIASES, normalize_host};

/// Binding lifetime bounds: `clamp(ttl, 10 min, 1 h)`.
pub const MIN_TTL_SECS: u32 = 600;
pub const MAX_TTL_SECS: u32 = 3600;
/// Longest CNAME chain followed.
const MAX_CHAIN: usize = MAX_ALIASES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsError {
    Parse,
}

/// One answer address with the name the device asked for and the CNAME chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub addr: IpAddr,
    /// Normalized query name.
    pub qname: String,
    /// Normalized CNAME targets in chain order.
    pub aliases: Vec<String>,
    /// Seconds, already clamped.
    pub ttl_secs: u32,
}

pub fn clamp_ttl(ttl: u32) -> u32 {
    ttl.clamp(MIN_TTL_SECS, MAX_TTL_SECS)
}

/// Bindings in a NOERROR response; queries and other responses yield none.
pub fn bindings(payload: &[u8]) -> Result<Vec<Binding>, DnsError> {
    let msg = Message::from_vec(payload).map_err(|_| DnsError::Parse)?;
    if msg.metadata.message_type != MessageType::Response
        || msg.metadata.response_code != ResponseCode::NoError
    {
        return Ok(Vec::new());
    }
    let Some(qname) = msg
        .queries
        .first()
        .and_then(|q| normalize_host(&q.name.to_ascii()))
    else {
        return Ok(Vec::new());
    };
    let mut cnames: HashMap<String, String> = HashMap::new();
    let mut addrs: Vec<(String, IpAddr, u32)> = Vec::new();
    for r in &msg.answers {
        let Some(owner) = normalize_host(&r.name.to_ascii()) else {
            continue;
        };
        match &r.data {
            RData::A(a) => addrs.push((owner, IpAddr::V4(a.0), r.ttl)),
            RData::AAAA(a) => addrs.push((owner, IpAddr::V6(a.0), r.ttl)),
            RData::CNAME(c) => {
                if let Some(target) = normalize_host(&c.0.to_ascii()) {
                    cnames.entry(owner).or_insert(target);
                }
            }
            _ => {}
        }
    }
    let chain = follow(&qname, &cnames);
    let reachable = |owner: &str| owner == qname || chain.iter().any(|c| c == owner);
    Ok(addrs
        .into_iter()
        .filter(|(owner, _, _)| reachable(owner))
        .map(|(_, addr, ttl)| Binding {
            addr,
            qname: qname.clone(),
            aliases: chain.clone(),
            ttl_secs: clamp_ttl(ttl),
        })
        .collect())
}

fn follow(qname: &str, cnames: &HashMap<String, String>) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur = qname;
    while let Some(next) = cnames.get(cur) {
        if chain.len() >= MAX_CHAIN || next == qname || chain.contains(next) {
            break;
        }
        chain.push(next.clone());
        cur = next;
    }
    chain
}
