mod common;

use common::raw;
use tmon_core::{Cidr, Direction, LocalNetworks};

#[test]
fn upload_when_src_local() {
    let nets = LocalNetworks::defaults();
    let r = raw("192.168.88.10", "1.1.1.1", 40000, 443, 6);
    let f = nets.to_record(&r);
    assert_eq!(f.direction, Direction::Up);
    assert_eq!(f.local_ip.to_string(), "192.168.88.10");
    assert_eq!(f.remote_port, 443);
}

#[test]
fn download_when_dst_local() {
    let nets = LocalNetworks::defaults();
    let f = nets.to_record(&raw("1.1.1.1", "10.1.2.3", 443, 40000, 6));
    assert_eq!(f.direction, Direction::Down);
    assert_eq!(f.local_ip.to_string(), "10.1.2.3");
    assert_eq!(f.remote_ip.to_string(), "1.1.1.1");
    assert_eq!(f.local_port, 40000);
    assert_eq!(f.remote_port, 443);
}

#[test]
fn internal_and_transit() {
    let nets = LocalNetworks::defaults();
    let internal = nets.to_record(&raw("172.16.0.5", "100.64.1.1", 1, 2, 17));
    assert_eq!(internal.direction, Direction::Internal);
    assert_eq!(internal.local_ip.to_string(), "172.16.0.5");
    let transit = nets.to_record(&raw("8.8.8.8", "9.9.9.9", 1, 2, 17));
    assert_eq!(transit.direction, Direction::Transit);
    assert_eq!(transit.local_ip.to_string(), "8.8.8.8");
}

#[test]
fn ipv6_ula_is_local() {
    let nets = LocalNetworks::defaults();
    let f = nets.to_record(&raw("2001:4860::8888", "fd00::42", 443, 5555, 17));
    assert_eq!(f.direction, Direction::Down);
    assert_eq!(f.ip_ver, 6);
    assert_eq!(f.local_ip.to_string(), "fd00::42");
}

#[test]
fn default_seed_networks() {
    let nets = LocalNetworks::defaults();
    for ip in [
        "10.0.0.1",
        "172.31.255.255",
        "192.168.0.1",
        "100.127.0.1",
        "fc00::1",
    ] {
        assert!(nets.contains(&ip.parse().unwrap()), "{ip} should be local");
    }
    for ip in ["172.32.0.1", "100.128.0.1", "8.8.8.8", "2001:db8::1"] {
        assert!(
            !nets.contains(&ip.parse().unwrap()),
            "{ip} should be remote"
        );
    }
}

#[test]
fn custom_networks() {
    let nets = LocalNetworks::new(vec!["203.0.113.0/24".parse::<Cidr>().unwrap()]);
    let f = nets.to_record(&raw("203.0.113.9", "192.168.1.1", 1, 2, 6));
    assert_eq!(f.direction, Direction::Up);
}

// ---- NAT (RouterOS exports on the WAN interface) ----

/// A download as RouterOS exports it at WAN ingress: addressed to the WAN IP,
/// with the LAN host only in postNATDestinationIPv4Address (IE 226/228).
fn wan_download(wan: &str) -> tmon_core::RawFlow {
    tmon_core::RawFlow {
        nat_dst_ip: Some("192.168.88.10".parse().unwrap()),
        nat_dst_port: Some(40000),
        ..raw("94.236.166.212", wan, 443, 61000, 6)
    }
}

#[test]
fn download_to_cgnat_wan_is_credited_to_lan_host() {
    let f = LocalNetworks::defaults().to_record(&wan_download("10.108.227.239"));
    assert_eq!(f.direction, Direction::Down);
    assert_eq!(f.local_ip.to_string(), "192.168.88.10");
    assert_eq!(f.local_port, 40000);
    assert_eq!(f.remote_ip.to_string(), "94.236.166.212");
    assert_eq!(f.remote_port, 443);
    assert_eq!(
        f.nat_ip.map(|i| i.to_string()).as_deref(),
        Some("10.108.227.239")
    );
    assert_eq!(f.nat_port, Some(61000));
}

#[test]
fn download_to_public_wan_is_down_not_transit() {
    let f = LocalNetworks::defaults().to_record(&wan_download("85.187.1.2"));
    assert_eq!(f.direction, Direction::Down);
    assert_eq!(f.local_ip.to_string(), "192.168.88.10");
    assert_eq!(
        f.nat_ip.map(|i| i.to_string()).as_deref(),
        Some("85.187.1.2")
    );
}

#[test]
fn upload_keeps_pre_nat_source_and_records_wan() {
    let r = tmon_core::RawFlow {
        nat_ip: Some("85.187.1.2".parse().unwrap()),
        nat_port: Some(61000),
        ..raw("192.168.88.10", "94.236.166.212", 40000, 443, 6)
    };
    let f = LocalNetworks::defaults().to_record(&r);
    assert_eq!(f.direction, Direction::Up);
    assert_eq!(f.local_ip.to_string(), "192.168.88.10");
    assert_eq!(f.local_port, 40000);
    assert_eq!(
        f.nat_ip.map(|i| i.to_string()).as_deref(),
        Some("85.187.1.2")
    );
    assert_eq!(f.nat_port, Some(61000));
}

#[test]
fn untranslated_nat_fields_are_ignored() {
    // RouterOS repeats the original address when no translation happened.
    let r = tmon_core::RawFlow {
        nat_ip: Some("94.236.166.212".parse().unwrap()),
        nat_port: Some(443),
        nat_dst_ip: Some("192.168.88.10".parse().unwrap()),
        nat_dst_port: Some(40000),
        ..raw("94.236.166.212", "192.168.88.10", 443, 40000, 6)
    };
    let f = LocalNetworks::defaults().to_record(&r);
    assert_eq!(f.direction, Direction::Down);
    assert_eq!(f.local_ip.to_string(), "192.168.88.10");
    assert_eq!((f.nat_ip, f.nat_port), (None, None));
}
