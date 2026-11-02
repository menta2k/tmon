//! Minimal IPv4/IPv6 CIDR type (parse, contains, display).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cidr {
    network: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Build a CIDR, masking host bits. Returns `None` when the prefix is too long.
    pub fn new(addr: IpAddr, prefix: u8) -> Option<Self> {
        let max = max_prefix(&addr);
        if prefix > max {
            return None;
        }
        Some(Self {
            network: mask(addr, prefix),
            prefix,
        })
    }

    /// A host route (/32 or /128).
    pub fn host(addr: IpAddr) -> Self {
        Self {
            network: addr,
            prefix: max_prefix(&addr),
        }
    }

    pub fn network(&self) -> IpAddr {
        self.network
    }

    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    pub fn contains(&self, ip: &IpAddr) -> bool {
        match (self.network, ip) {
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)) => {
                mask(*ip, self.prefix) == self.network
            }
            _ => false,
        }
    }

    /// Numeric `(bits, len)` form used by prefix tries.
    pub fn v4_key(&self) -> Option<(u32, u8)> {
        match self.network {
            IpAddr::V4(a) => Some((u32::from(a), self.prefix)),
            IpAddr::V6(_) => None,
        }
    }

    pub fn v6_key(&self) -> Option<(u128, u8)> {
        match self.network {
            IpAddr::V6(a) => Some((u128::from(a), self.prefix)),
            IpAddr::V4(_) => None,
        }
    }
}

fn max_prefix(addr: &IpAddr) -> u8 {
    if addr.is_ipv4() { 32 } else { 128 }
}

fn mask(addr: IpAddr, prefix: u8) -> IpAddr {
    match addr {
        IpAddr::V4(a) => {
            let bits = u32::from(a);
            let m = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - u32::from(prefix))
            };
            IpAddr::V4((bits & m).into())
        }
        IpAddr::V6(a) => {
            let bits = u128::from(a);
            let m = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(prefix))
            };
            IpAddr::V6((bits & m).into())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid CIDR")]
pub struct CidrParseError;

impl FromStr for Cidr {
    type Err = CidrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        match s.split_once('/') {
            Some((ip, len)) => {
                let ip: IpAddr = ip.parse().map_err(|_| CidrParseError)?;
                let len: u8 = len.parse().map_err(|_| CidrParseError)?;
                Cidr::new(ip, len).ok_or(CidrParseError)
            }
            None => s
                .parse::<IpAddr>()
                .map(Cidr::host)
                .map_err(|_| CidrParseError),
        }
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix)
    }
}

impl Serialize for Cidr {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Cidr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_masks() {
        let c: Cidr = "192.168.1.77/24".parse().unwrap();
        assert_eq!(c.to_string(), "192.168.1.0/24");
        assert!(c.contains(&"192.168.1.200".parse().unwrap()));
        assert!(!c.contains(&"192.168.2.1".parse().unwrap()));
        assert!(!c.contains(&"::1".parse().unwrap()));
    }

    #[test]
    fn host_and_v6() {
        let c: Cidr = "fc00::/7".parse().unwrap();
        assert!(c.contains(&"fd12::1".parse().unwrap()));
        let h: Cidr = "10.0.0.1".parse().unwrap();
        assert_eq!(h.prefix(), 32);
        assert!("10.0.0.0/33".parse::<Cidr>().is_err());
        assert!("nope".parse::<Cidr>().is_err());
        let zero: Cidr = "0.0.0.0/0".parse().unwrap();
        assert!(zero.contains(&"8.8.8.8".parse().unwrap()));
    }
}
