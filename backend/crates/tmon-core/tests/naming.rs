use std::sync::Arc;
use tmon_core::naming::{
    ConnKey, ConnName, MAX_ALIASES, NameSource, normalize_alpn, normalize_host,
};
use tmon_core::{Direction, FlowRecord, RawFlow};

fn record() -> FlowRecord {
    let raw = RawFlow {
        ts_ms: 1,
        ts_start_ms: 1,
        exporter_id: 1,
        in_if: 0,
        out_if: 0,
        src_ip: "192.168.1.5".parse().unwrap(),
        dst_ip: "142.250.1.1".parse().unwrap(),
        src_port: 51000,
        dst_port: 443,
        proto: 6,
        tcp_flags: 0,
        bytes: 1,
        packets: 1,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    };
    FlowRecord::from_raw(&raw, Direction::Up, true)
}

#[test]
fn conn_key_maps_local_to_lan() {
    let k = ConnKey::from_record(&record());
    assert_eq!(k.lan_ip.to_string(), "192.168.1.5");
    assert_eq!(k.lan_port, 51000);
    assert_eq!(k.remote_ip.to_string(), "142.250.1.1");
    assert_eq!(k.remote_port, 443);
    assert_eq!(k.proto, 6);
}

#[test]
fn name_source_round_trips() {
    for (code, s) in [
        (1, "tls"),
        (2, "quic"),
        (3, "http"),
        (4, "dns"),
        (5, "router"),
    ] {
        let src = NameSource::from_code(code).unwrap();
        assert_eq!(src.code(), code);
        assert_eq!(src.as_str(), s);
        assert_eq!(NameSource::parse(s), Some(src));
        assert_eq!(serde_json::to_string(&src).unwrap(), format!("\"{s}\""));
    }
    assert_eq!(NameSource::from_code(0), None);
    assert_eq!(NameSource::from_code(7), None);
    assert_eq!(NameSource::parse("x"), None);
}

#[test]
fn hosts_are_normalized() {
    assert_eq!(
        normalize_host("WWW.YouTube.com.").as_deref(),
        Some("www.youtube.com")
    );
    assert_eq!(
        normalize_host("_dmarc.x-y.example").as_deref(),
        Some("_dmarc.x-y.example")
    );
    assert_eq!(
        normalize_host("xn--bcher-kva.example").as_deref(),
        Some("xn--bcher-kva.example")
    );
    assert_eq!(normalize_host("localhost").as_deref(), Some("localhost"));
}

#[test]
fn all_numeric_names_are_rejected() {
    assert_eq!(normalize_host("192.168.1.1"), None);
    assert_eq!(normalize_host("1234"), None);
    assert_eq!(normalize_host("1.2.3.4."), None);
    assert_eq!(normalize_host("1e100.net").as_deref(), Some("1e100.net"));
}

#[test]
fn invalid_hosts_are_rejected() {
    let long_label = format!("{}.com", "a".repeat(64));
    let long_host = vec!["a".repeat(63); 5].join(".");
    assert!(long_host.len() > 253);
    for bad in [
        "",
        ".",
        "a..b",
        "bücher.de",
        "a b.com",
        "a/b",
        "a:443",
        &long_label,
        &long_host,
    ] {
        assert_eq!(normalize_host(bad), None, "{bad:?}");
    }
    assert!(normalize_host(&vec!["a".repeat(63); 3].join(".")).is_some());
}

#[test]
fn alpn_is_validated() {
    assert_eq!(normalize_alpn(b"h2").as_deref(), Some("h2"));
    assert_eq!(normalize_alpn(b"http/1.1").as_deref(), Some("http/1.1"));
    assert_eq!(normalize_alpn(&[b'a'; 32]).map(|a| a.len()), Some(32));
    assert_eq!(normalize_alpn(&[b'a'; 33]), None);
    assert_eq!(normalize_alpn(b""), None);
    assert_eq!(normalize_alpn(b"h 2"), None);
    assert_eq!(normalize_alpn(&[0xff, 0x01]), None);
}

#[test]
fn conn_name_builders_are_immutable_and_bounded() {
    let base = ConnName::new("Example.COM", NameSource::Dns).unwrap();
    let aliases: Vec<String> = (0..12).map(|i| format!("cdn{i}.example.net")).collect();
    let with = base
        .clone()
        .with_aliases(&aliases)
        .with_alpn(Some(Arc::from("h3")))
        .with_ech_outer(true);
    assert_eq!(&*base.host, "example.com");
    assert!(base.aliases.is_empty());
    assert_eq!(with.aliases.len(), MAX_ALIASES);
    assert_eq!(with.alpn.as_deref(), Some("h3"));
    assert!(with.ech_outer);
    let dedup = base.with_aliases(&["example.com", "a.net", "A.net.", "bad host"]);
    assert_eq!(
        dedup.aliases.iter().map(|a| &**a).collect::<Vec<_>>(),
        ["a.net"]
    );
    assert!(ConnName::new("bad host", NameSource::Tls).is_err());
}
