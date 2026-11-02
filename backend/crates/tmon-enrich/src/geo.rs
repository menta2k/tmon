//! GeoIP / ASN lookup abstraction.

use std::net::IpAddr;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GeoInfo {
    pub country: Option<Arc<str>>,
    pub asn: Option<u32>,
    pub as_name: Option<Arc<str>>,
}

/// Database load state reported by `/health` and `/status`.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct GeoStatus {
    pub loaded: bool,
    /// Build date of the loaded database(s), e.g. `2026-10-01`.
    pub db_date: Option<String>,
}

/// Country/ASN enrichment for public addresses.
pub trait GeoLookup: Send + Sync {
    fn lookup(&self, ip: IpAddr) -> GeoInfo;

    fn status(&self) -> GeoStatus {
        GeoStatus::default()
    }
}

/// No-op lookup used when no databases are configured.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoGeo;

impl GeoLookup for NoGeo {
    fn lookup(&self, _ip: IpAddr) -> GeoInfo {
        GeoInfo::default()
    }
}

/// True for addresses that never carry geo/ASN data (private, reserved, ULA, link-local…).
pub fn is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            a.is_private()
                || a.is_loopback()
                || a.is_link_local()
                || a.is_broadcast()
                || a.is_unspecified()
                || a.is_multicast()
                || a.is_documentation()
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || o[0] >= 240
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ranges() {
        for ip in [
            "10.1.1.1",
            "192.168.0.1",
            "100.64.0.1",
            "169.254.1.1",
            "fd00::1",
            "fe80::1",
            "127.0.0.1",
            "203.0.113.5",
            "2001:db8::1",
        ] {
            assert!(is_private(&ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700::1111"] {
            assert!(!is_private(&ip.parse().unwrap()), "{ip}");
        }
        assert_eq!(NoGeo.lookup("8.8.8.8".parse().unwrap()), GeoInfo::default());
    }
}
