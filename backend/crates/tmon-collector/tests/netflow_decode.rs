use std::path::PathBuf;
use tmon_collector::fixtures::corpus::{BASE_MS, basic_flows};
use tmon_collector::pcap::{UdpPacket, read_udp};
use tmon_collector::{ExporterRegistry, NetflowDecoder};
use tmon_core::RawFlow;

fn fixture(name: &str) -> Vec<UdpPacket> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    read_udp(std::fs::File::open(&p).expect("fixture exists")).expect("valid pcap")
}

fn decode_all(name: &str, now_ms: i64) -> (Vec<RawFlow>, ExporterRegistry) {
    let reg = ExporterRegistry::new();
    let mut dec = NetflowDecoder::new();
    let flows = fixture(name)
        .iter()
        .flat_map(|p| dec.decode(p.src, &p.payload, &reg, now_ms).flows)
        .collect();
    (flows, reg)
}

fn assert_basic(flows: &[RawFlow], expect_v6: bool) {
    let expected: Vec<_> = basic_flows()
        .into_iter()
        .filter(|f| expect_v6 || f.src.is_ipv4())
        .collect();
    assert_eq!(flows.len(), expected.len());
    for (got, want) in flows.iter().zip(expected.iter()) {
        assert_eq!((got.src_ip, got.dst_ip), (want.src, want.dst));
        assert_eq!(
            (got.src_port, got.dst_port, got.proto),
            (want.sport, want.dport, want.proto)
        );
        assert_eq!((got.bytes, got.packets), (want.bytes, want.packets));
        assert_eq!(
            (got.ts_start_ms, got.ts_ms),
            (want.start_ms, want.end_ms),
            "timestamps"
        );
        assert_eq!((got.in_if, got.out_if), (1, 2));
        assert!(!got.sampled);
    }
}

#[test]
fn decodes_netflow_v5() {
    let (flows, reg) = decode_all("nf5.pcap", BASE_MS);
    assert_basic(&flows, false);
    assert_eq!(flows[0].tcp_flags, 0x1b);
    assert_eq!(reg.all()[0].protocol().as_str(), "netflow5");
}

#[test]
fn decodes_netflow_v9() {
    let (flows, reg) = decode_all("nf9.pcap", BASE_MS);
    assert_basic(&flows, true);
    let e = &reg.all()[0];
    assert_eq!((e.domain_id, e.protocol().as_str()), (7, "netflow9"));
    assert_eq!(e.counters.snapshot().decoded, 4);
}

#[test]
fn decodes_routeros_ipfix_with_nat() {
    let (flows, reg) = decode_all("routeros7-ipfix.pcap", BASE_MS);
    assert_eq!(flows.len(), 4);
    assert_eq!(reg.all()[0].protocol().as_str(), "ipfix");

    let up = flows
        .iter()
        .find(|f| f.src_ip.to_string() == "192.168.88.10")
        .unwrap();
    assert_eq!(
        up.nat_ip.map(|i| i.to_string()).as_deref(),
        Some("100.70.0.2")
    );
    assert_eq!(up.nat_port, Some(61000));
    assert_eq!(
        up.nat_dst_ip, None,
        "untranslated destination is not reported as NAT"
    );

    // RouterOS on the WAN: the download is addressed to the WAN IP and the LAN
    // host is only in postNATDestination (IE 226/228).
    let down = flows
        .iter()
        .find(|f| f.src_ip.to_string() == "203.0.113.10")
        .unwrap();
    assert_eq!(down.dst_ip.to_string(), "100.70.0.2");
    assert_eq!(
        down.nat_dst_ip.map(|i| i.to_string()).as_deref(),
        Some("192.168.88.10")
    );
    assert_eq!(down.nat_dst_port, Some(40000));
    assert_eq!(
        down.nat_ip, None,
        "untranslated source is not reported as NAT"
    );

    let rec = tmon_core::LocalNetworks::defaults().to_record(down);
    assert_eq!(rec.direction, tmon_core::Direction::Down);
    assert_eq!(
        (rec.local_ip.to_string(), rec.local_port),
        ("192.168.88.10".to_string(), 40000)
    );
    assert_eq!(
        rec.nat_ip.map(|i| i.to_string()).as_deref(),
        Some("100.70.0.2")
    );
}

#[test]
fn data_before_template_is_counted_then_decoded() {
    let (flows, reg) = decode_all("nf9-data-before-template.pcap", BASE_MS);
    let c = reg.all()[0].counters.snapshot();
    assert!(c.template_missing > 0, "template_missing must be counted");
    assert_eq!(flows.len(), 4, "records after the template decode");
    assert_eq!(c.received, 2);
}

#[test]
fn malformed_datagrams_count_errors_without_panicking() {
    let reg = ExporterRegistry::new();
    let mut dec = NetflowDecoder::new();
    let mut unknown = 0;
    let mut flows = 0;
    for p in fixture("malformed.pcap") {
        let d = dec.decode(p.src, &p.payload, &reg, BASE_MS);
        unknown += usize::from(d.unknown_version);
        flows += d.flows.len();
    }
    let c = reg.all()[0].counters.snapshot();
    assert!(c.parse_errors >= 2, "parse errors counted: {c:?}");
    assert_eq!(
        unknown, 1,
        "the 0x4242 garbage datagram has an unknown version"
    );
    assert_eq!(flows, 4, "the trailing valid message still decodes");
}

#[test]
fn skewed_exporter_clock_falls_back_to_receive_time() {
    let now = BASE_MS + 3_600_000;
    let (flows, _) = decode_all("nf9.pcap", now);
    for f in &flows {
        assert_eq!(f.ts_ms, now);
        assert_eq!(f.ts_ms - f.ts_start_ms, 29_000, "duration kept");
    }
}

#[test]
fn sampling_override_scales_counts() {
    let reg = ExporterRegistry::new();
    let mut dec = NetflowDecoder::new();
    let pkts = fixture("nf9.pcap");
    let first = dec.decode(pkts[0].src, &pkts[0].payload, &reg, BASE_MS);
    let id = reg.all()[0].id;
    let settings = tmon_collector::ExporterSettings {
        sampling_override: Some(10),
        ..Default::default()
    };
    reg.update_settings(id, settings);
    let second = dec.decode(pkts[0].src, &pkts[0].payload, &reg, BASE_MS);
    assert_eq!(second.flows[0].bytes, first.flows[0].bytes * 10);
    assert!(second.flows[0].sampled);
}

/// Real exporters keep datagrams within a 1500-byte MTU; larger ones need IP
/// fragmentation, which some networks (e.g. WSL mirrored mode) silently drop.
#[test]
fn fixture_datagrams_fit_a_standard_mtu() {
    const MAX_UDP_PAYLOAD: usize = 1500 - 20 - 8;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if !name.ends_with(".pcap") || name == "malformed.pcap" {
            continue;
        }
        for p in fixture(&name) {
            assert!(
                p.payload.len() <= MAX_UDP_PAYLOAD,
                "{name}: {}-byte datagram",
                p.payload.len()
            );
        }
    }
}
