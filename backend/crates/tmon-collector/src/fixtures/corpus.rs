//! Deterministic fixture corpus (see backend/tests/fixtures/README.md).

use std::fs;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use super::encode::{FlowSpec, encode_ipfix, encode_sflow, encode_v5, encode_v9};
use crate::pcap::{UdpPacket, write_udp};

/// 2026-01-01T00:00:00Z — base time of all fixtures.
pub const BASE_MS: i64 = 1_767_225_600_000;
pub const EXPORTER: &str = "192.168.88.1";
pub const SFLOW_RATE: u32 = 512;
const DST: &str = "127.0.0.1:2055";
const SFLOW_DST: &str = "127.0.0.1:6343";
/// Records per IPFIX datagram: keeps every datagram within a 1500-byte MTU, like real exporters.
const PER_PACKET: usize = 16;

/// One flow of the "mixed home" scenario with its expected category name.
#[derive(Debug, Clone)]
pub struct TruthFlow {
    pub spec: FlowSpec,
    pub category: &'static str,
    pub direction: &'static str,
    pub local_ip: IpAddr,
}

fn ip(s: &str) -> IpAddr {
    s.parse().expect("valid fixture ip")
}

pub fn flow(src: &str, dst: &str, sport: u16, dport: u16, proto: u8, bytes: u64) -> FlowSpec {
    FlowSpec {
        start_ms: BASE_MS - 30_000,
        end_ms: BASE_MS - 1_000,
        src: ip(src),
        dst: ip(dst),
        sport,
        dport,
        proto,
        tcp_flags: if proto == 6 { 0x1b } else { 0 },
        bytes,
        packets: bytes.div_ceil(1200).max(1),
        in_if: 1,
        out_if: 2,
        nat: None,
        nat_dst: None,
    }
}

/// Mixed home traffic: (local host, remote, remote port, proto, bytes, category).
const MIXED: &[(&str, &str, u16, u8, u64, &str)] = &[
    ("192.168.88.10", "203.0.113.10", 443, 6, 50_000_000, "Web"),
    ("192.168.88.10", "198.51.100.53", 53, 17, 120_000, "DNS"),
    (
        "192.168.88.11",
        "203.0.113.20",
        27020,
        17,
        8_000_000,
        "Gaming",
    ),
    ("192.168.88.11", "203.0.113.21", 443, 17, 30_000_000, "Web"),
    (
        "192.168.88.12",
        "198.51.100.7",
        51820,
        17,
        4_000_000,
        "VPN/Tunnels",
    ),
    ("192.168.88.12", "198.51.100.25", 993, 6, 900_000, "Email"),
    ("192.168.88.13", "203.0.113.99", 9999, 6, 2_000_000, "Other"),
    (
        "192.168.88.13",
        "198.51.100.123",
        123,
        17,
        9_000,
        "Network Services",
    ),
    ("fd00::10", "2001:db8::443", 443, 6, 12_000_000, "Web"),
    // Resolvable in the MaxMind test DBs (SE, AS29518) so drill-downs show real geo/ASN.
    ("192.168.88.10", "89.160.20.112", 443, 6, 20_000_000, "Web"),
];

/// Each entry yields a download flow (remote → local) and an upload flow (10 %).
pub fn mixed_home() -> Vec<TruthFlow> {
    let mut out = Vec::new();
    for (i, (local, remote, rport, proto, bytes, cat)) in MIXED.iter().enumerate() {
        let lport = 40_000 + i as u16;
        out.push(TruthFlow {
            spec: flow(remote, local, *rport, lport, *proto, *bytes),
            category: cat,
            direction: "down",
            local_ip: ip(local),
        });
        out.push(TruthFlow {
            spec: flow(local, remote, lport, *rport, *proto, bytes / 10),
            category: cat,
            direction: "up",
            local_ip: ip(local),
        });
    }
    out
}

/// Steam traffic on UDP 27015–27030 (for custom rule tests).
pub fn steam() -> Vec<FlowSpec> {
    (0..10u16)
        .map(|i| {
            flow(
                "203.0.113.77",
                "192.168.88.20",
                27015 + i,
                50_000 + i,
                17,
                1_000_000,
            )
        })
        .collect()
}

fn udp(ts_ms: i64, src: &str, dst: &str, payload: Vec<u8>) -> UdpPacket {
    UdpPacket {
        ts_us: ts_ms * 1000,
        src: SocketAddr::new(ip(src), 4739),
        dst: dst.parse().expect("valid fixture dst"),
        payload,
    }
}

/// IPFIX datagrams (template in the first) carrying `flows`.
pub fn ipfix_packets(flows: &[FlowSpec]) -> Vec<UdpPacket> {
    flows
        .chunks(PER_PACKET)
        .enumerate()
        .map(|(i, c)| {
            let ts = BASE_MS + i as i64 * 10;
            udp(ts, EXPORTER, DST, encode_ipfix(ts, i as u32, 0, i == 0, c))
        })
        .collect()
}

fn write(dir: &Path, name: &str, pkts: &[UdpPacket]) -> io::Result<()> {
    write_udp(fs::File::create(dir.join(name))?, pkts)
}

/// Generate every fixture file into `dir`.
pub fn generate(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let base = basic_flows();
    let v4: Vec<FlowSpec> = base.iter().filter(|f| f.src.is_ipv4()).cloned().collect();
    write(
        dir,
        "nf5.pcap",
        &[udp(BASE_MS, EXPORTER, DST, encode_v5(BASE_MS, 1, 0, &v4))],
    )?;
    write(
        dir,
        "nf9.pcap",
        &[udp(
            BASE_MS,
            EXPORTER,
            DST,
            encode_v9(BASE_MS, 1, 7, true, &base),
        )],
    )?;
    write(
        dir,
        "routeros7-ipfix.pcap",
        &ipfix_packets(&with_nat(&base)),
    )?;
    let late = vec![
        udp(
            BASE_MS,
            EXPORTER,
            DST,
            encode_v9(BASE_MS, 1, 7, false, &base),
        ),
        udp(
            BASE_MS + 10,
            EXPORTER,
            DST,
            encode_v9(BASE_MS + 10, 2, 7, true, &base),
        ),
    ];
    write(dir, "nf9-data-before-template.pcap", &late)?;
    write(dir, "malformed.pcap", &malformed())?;
    let sampled: Vec<FlowSpec> = base
        .iter()
        .map(|f| FlowSpec {
            bytes: 1500,
            ..f.clone()
        })
        .collect();
    let sflow = encode_sflow(ip(EXPORTER), 1, SFLOW_RATE, &sampled);
    write(
        dir,
        "sflow-crs-1in512.pcap",
        &[udp(BASE_MS, EXPORTER, SFLOW_DST, sflow)],
    )?;
    let mixed: Vec<FlowSpec> = mixed_home().into_iter().map(|t| t.spec).collect();
    write(dir, "mixed-home.pcap", &ipfix_packets(&mixed))?;
    fs::write(dir.join("mixed-home.truth.csv"), truth_csv(&mixed_home()))?;
    write(dir, "steam.pcap", &ipfix_packets(&steam()))
}

/// Four basic flows: v4 TCP down, v4 UDP up, v4 ICMP, v6 TCP down.
pub fn basic_flows() -> Vec<FlowSpec> {
    vec![
        flow("203.0.113.10", "192.168.88.10", 443, 40000, 6, 1_500_000),
        flow("192.168.88.10", "198.51.100.53", 40001, 53, 17, 400),
        flow("192.168.88.11", "198.51.100.1", 0, 0, 1, 840),
        flow("2001:db8::1", "fd00::10", 443, 40002, 6, 2_000_000),
    ]
}

/// RouterOS exporting on the WAN (CGNAT address 100.70.0.2): uploads are
/// source-NATed, downloads arrive addressed to the WAN and are un-NATed to the LAN host.
fn with_nat(flows: &[FlowSpec]) -> Vec<FlowSpec> {
    const WAN: (&str, u16) = ("100.70.0.2", 61000);
    let is_lan = |a: &IpAddr| a.is_ipv4() && a.to_string().starts_with("192.168.");
    flows
        .iter()
        .map(|f| match (is_lan(&f.src), is_lan(&f.dst)) {
            (true, false) => FlowSpec {
                nat: Some((ip(WAN.0), WAN.1)),
                ..f.clone()
            },
            (false, true) => FlowSpec {
                dst: ip(WAN.0),
                dport: WAN.1,
                nat_dst: Some((f.dst, f.dport)),
                ..f.clone()
            },
            _ => f.clone(),
        })
        .collect()
}

fn malformed() -> Vec<UdpPacket> {
    let good = encode_ipfix(BASE_MS, 1, 0, true, &basic_flows());
    let truncated = good[..good.len() / 2].to_vec();
    let mut bad_len = good.clone();
    bad_len[2..4].copy_from_slice(&0xffffu16.to_be_bytes());
    vec![
        udp(BASE_MS, EXPORTER, DST, vec![0, 9, 0xff]),
        udp(BASE_MS, EXPORTER, DST, truncated),
        udp(BASE_MS, EXPORTER, DST, bad_len),
        udp(BASE_MS, EXPORTER, DST, vec![0x42; 64]),
        udp(BASE_MS + 1, EXPORTER, DST, good),
    ]
}

/// CSV `category,direction,local_ip,bytes` aggregated per key.
pub fn truth_csv(flows: &[TruthFlow]) -> String {
    let mut rows: std::collections::BTreeMap<(String, String, String), u64> = Default::default();
    for t in flows {
        let key = (
            t.category.to_string(),
            t.direction.to_string(),
            t.local_ip.to_string(),
        );
        *rows.entry(key).or_default() += t.spec.bytes;
    }
    let mut out = String::from("category,direction,local_ip,bytes\n");
    for ((c, d, l), b) in rows {
        out.push_str(&format!("{c},{d},{l},{b}\n"));
    }
    out
}
