use std::path::PathBuf;
use tmon_collector::fixtures::corpus::{BASE_MS, SFLOW_RATE, basic_flows};
use tmon_collector::pcap::read_udp;
use tmon_collector::{ExporterRegistry, SflowDecoder};

#[test]
fn decodes_sampled_headers_and_scales() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/sflow-crs-1in512.pcap");
    let pkts = read_udp(std::fs::File::open(p).unwrap()).unwrap();
    let reg = ExporterRegistry::new();
    let dec = SflowDecoder::new();
    let d = dec.decode(pkts[0].src, &pkts[0].payload, &reg, BASE_MS);
    let expected = basic_flows();
    assert_eq!(d.flows.len(), expected.len());
    for (got, want) in d.flows.iter().zip(expected.iter()) {
        assert_eq!(
            (got.src_ip, got.dst_ip, got.proto),
            (want.src, want.dst, want.proto)
        );
        assert_eq!((got.src_port, got.dst_port), (want.sport, want.dport));
        assert_eq!(got.bytes, 1500 * u64::from(SFLOW_RATE));
        assert_eq!(got.packets, u64::from(SFLOW_RATE));
        assert!(got.sampled);
        assert_eq!(got.ts_ms, BASE_MS);
    }
    assert!(
        d.flows.iter().any(|f| f.src_ip.is_ipv6()),
        "IPv6 header decoded"
    );
    assert_eq!(d.flows[0].tcp_flags, 0x1b);
    let e = &reg.all()[0];
    assert_eq!(e.protocol().as_str(), "sflow5");
    assert_eq!(e.sampling_rate(), SFLOW_RATE);
    assert_eq!(e.address.to_string(), "192.168.88.1");
}

#[test]
fn garbage_is_a_parse_error() {
    let reg = ExporterRegistry::new();
    let d = SflowDecoder::new().decode("10.0.0.1:6343".parse().unwrap(), &[0, 0, 0, 5, 1], &reg, 0);
    assert!(d.flows.is_empty());
    assert_eq!(reg.all()[0].counters.snapshot().parse_errors, 1);
}
