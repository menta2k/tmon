use std::path::PathBuf;
use tmon_enrich::GeoLookup;
use tmon_enrich::geoip::GeoIp;

fn geo_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/geo")
}

fn open() -> std::sync::Arc<GeoIp> {
    GeoIp::open(
        &geo_dir().join("GeoIP2-Country-Test.mmdb"),
        &geo_dir().join("GeoLite2-ASN-Test.mmdb"),
    )
}

#[test]
fn known_ip_resolves_country_and_asn() {
    let g = open();
    assert!(g.status().loaded);
    assert!(g.status().db_date.is_some());
    let c = g.lookup("81.2.69.160".parse().unwrap());
    assert_eq!(c.country.as_deref(), Some("GB"));
    let a = g.lookup("1.128.0.0".parse().unwrap());
    assert_eq!(a.asn, Some(1221));
    assert_eq!(a.as_name.as_deref(), Some("Telstra Pty Ltd"));
}

#[test]
fn private_addresses_are_skipped() {
    let g = open();
    for ip in ["192.168.1.1", "10.0.0.1", "fd00::1", "fe80::1"] {
        assert_eq!(g.lookup(ip.parse().unwrap()), Default::default(), "{ip}");
    }
    assert_eq!(g.cached(), 0, "private lookups are not cached");
}

#[test]
fn missing_database_degrades_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let g = GeoIp::open(
        &dir.path().join("none.mmdb"),
        &dir.path().join("none2.mmdb"),
    );
    assert!(!g.status().loaded);
    assert_eq!(g.lookup("81.2.69.160".parse().unwrap()), Default::default());
}

#[test]
fn replaced_file_is_hot_reloaded_and_cache_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let (c, a) = (dir.path().join("c.mmdb"), dir.path().join("a.mmdb"));
    let g = GeoIp::open(&c, &a);
    assert!(!g.status().loaded);
    g.lookup("81.2.69.160".parse().unwrap());
    assert_eq!(g.cached(), 1);
    std::fs::copy(geo_dir().join("GeoIP2-Country-Test.mmdb"), &c).unwrap();
    std::fs::copy(geo_dir().join("GeoLite2-ASN-Test.mmdb"), &a).unwrap();
    assert!(g.reload_if_changed());
    assert_eq!(g.cached(), 0);
    assert!(g.status().loaded);
    assert_eq!(
        g.lookup("81.2.69.160".parse().unwrap()).country.as_deref(),
        Some("GB")
    );
    assert!(!g.reload_if_changed(), "unchanged files are not reloaded");
}

#[test]
fn lru_hit_path_returns_same_answer() {
    let g = open();
    let ip = "89.160.20.112".parse().unwrap();
    let first = g.lookup(ip);
    assert_eq!(g.cached(), 1);
    assert_eq!(g.lookup(ip), first);
    assert_eq!(g.cached(), 1);
}
