//! Named capture scenarios: the committed pcaps, the integration tests and
//! the fuzz seeds all come from here (quickstart V2–V7).

use hickory_proto::op::ResponseCode;
use std::net::{IpAddr, SocketAddr};

use super::dns::{Answer, response};
use super::hello::{Hello, client_hello, record};
use super::quic::{client_initial, crypto_frame};
use super::{SYN, addr, tcp_stream, udp};
use crate::quic::Version;

/// Where the router sends DNS answers from.
pub const RESOLVER: &str = "192.168.88.1:53";
const MSS: usize = 1400;
const QUIC_PAYLOAD: usize = 1162;

/// A connection a scenario names, and what tmon should show for it.
#[derive(Debug, Clone, Copy)]
pub struct Expect {
    pub scenario: &'static str,
    pub client: &'static str,
    pub server: &'static str,
    pub proto: u8,
    pub host: &'static str,
    pub source: &'static str,
    /// Category the built-in rules give the named flow.
    pub category: &'static str,
}

/// Every named connection of the US1/US2 scenarios.
pub const EXPECTED: &[Expect] = &[
    Expect {
        scenario: "tls_split_pq",
        client: "192.168.88.10:51000",
        server: "142.250.1.1:443",
        proto: 6,
        host: "rr3.googlevideo.com",
        source: "tls",
        category: "Streaming",
    },
    Expect {
        scenario: "tls_ech",
        client: "192.168.88.10:51001",
        server: "104.16.1.1:443",
        proto: 6,
        host: "cloudflare-ech.com",
        source: "tls",
        category: "Web",
    },
    Expect {
        scenario: "quic_v1_rfc",
        client: "192.168.88.10:55001",
        server: "93.184.215.14:443",
        proto: 17,
        host: "example.com",
        source: "quic",
        category: "Web",
    },
    Expect {
        scenario: "quic_v1_reordered",
        client: "192.168.88.11:55002",
        server: "104.18.32.47:443",
        proto: 17,
        host: "chatgpt.com",
        source: "quic",
        category: "AI Services",
    },
    Expect {
        scenario: "quic_v2_rfc",
        client: "192.168.88.11:55003",
        server: "93.184.215.15:443",
        proto: 17,
        host: "example.com",
        source: "quic",
        category: "Web",
    },
    Expect {
        scenario: "http_host",
        client: "192.168.88.12:40000",
        server: "34.223.124.45:80",
        proto: 6,
        host: "neverssl.com",
        source: "http",
        category: "Web",
    },
    Expect {
        scenario: "shared_cdn",
        client: "192.168.88.10:52000",
        server: "151.101.1.1:443",
        proto: 6,
        host: "www.netflix.com",
        source: "tls",
        category: "Streaming",
    },
    Expect {
        scenario: "shared_cdn",
        client: "192.168.88.11:52001",
        server: "151.101.1.1:443",
        proto: 6,
        host: "www.reddit.com",
        source: "tls",
        category: "Social",
    },
    Expect {
        scenario: "dns_cname",
        client: "192.168.88.13:40100",
        server: "142.250.200.1:8443",
        proto: 6,
        host: "media.example.org",
        source: "dns",
        category: "Streaming",
    },
];

/// Flow whose DNS answer arrives only after the flow (back-fill, US2).
pub const LATE: Expect = Expect {
    scenario: "dns_late",
    client: "192.168.88.13:40101",
    server: "142.250.200.2:8443",
    proto: 6,
    host: "late.example.org",
    source: "dns",
    category: "Other",
};

fn tls_hello(sni: &str, alpn: &[&str], pq: bool, ech: bool) -> Vec<u8> {
    record(&client_hello(&Hello {
        sni: Some(sni),
        alpn,
        ech,
        pq_key_share: pq,
    }))
}

fn tls_conn(e: &Expect, pq: bool, ech: bool, isn: u32) -> Vec<Vec<u8>> {
    let hello = tls_hello(e.host, &["h2", "http/1.1"], pq, ech);
    tcp_stream(addr(e.client), addr(e.server), isn, &hello, MSS)
}

fn find(scenario: &str) -> Vec<&'static Expect> {
    EXPECTED.iter().filter(|e| e.scenario == scenario).collect()
}

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(char::is_ascii_hexdigit).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn ip(s: &str) -> IpAddr {
    s.parse().expect("fixture ip")
}

/// QUIC hello split into `parts` CRYPTO frames, one Initial each, last first.
fn quic_reordered(e: &Expect, version: Version, parts: usize) -> Vec<Vec<u8>> {
    let hello = client_hello(&Hello {
        sni: Some(e.host),
        alpn: &["h3"],
        ech: false,
        pq_key_share: true,
    });
    let size = hello.len().div_ceil(parts);
    let dcid = [0x5a, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
    let mut pkts: Vec<Vec<u8>> = hello
        .chunks(size)
        .enumerate()
        .map(|(i, c)| {
            client_initial(
                version,
                &dcid,
                &[1, 2, 3, 4],
                i as u32,
                &crypto_frame((i * size) as u64, c),
                QUIC_PAYLOAD,
            )
        })
        .map(|p| udp(addr(e.client), addr(e.server), &p))
        .collect();
    pkts.reverse();
    pkts
}

fn dns_answer(to: SocketAddr, payload: &[u8]) -> Vec<u8> {
    udp(addr(RESOLVER), to, payload)
}

/// The TZSP datagrams of one scenario, in send order.
pub fn datagrams(name: &str) -> Vec<Vec<u8>> {
    let first = || find(name)[0];
    match name {
        "tls_split_pq" => tls_conn(first(), true, false, 1_000),
        "tls_ech" => tls_conn(first(), false, true, 2_000),
        "quic_v1_rfc" => vec![udp(
            addr(first().client),
            addr(first().server),
            &hex(include_str!("../../tests/vectors/rfc9001_packet.hex")),
        )],
        "quic_v2_rfc" => vec![udp(
            addr(first().client),
            addr(first().server),
            &hex(include_str!("../../tests/vectors/rfc9369_packet.hex")),
        )],
        "quic_v1_reordered" => quic_reordered(first(), Version::V1, 3),
        "http_host" => {
            let e = first();
            let req = format!(
                "GET / HTTP/1.1\r\nHost: {}\r\nUser-Agent: curl/8\r\n\r\n",
                e.host
            );
            tcp_stream(addr(e.client), addr(e.server), 3_000, req.as_bytes(), MSS)
        }
        "shared_cdn" => find(name)
            .iter()
            .flat_map(|e| tls_conn(e, false, false, 4_000))
            .collect(),
        "dns_cname" => {
            let e = first();
            let ans = response(
                e.host,
                &[
                    Answer::Cname(e.host, "rr5.googlevideo.com", 300),
                    Answer::Addr("rr5.googlevideo.com", ip("142.250.200.1"), 300),
                ],
                ResponseCode::NoError,
            );
            vec![dns_answer(SocketAddr::new(addr(e.client).ip(), 5353), &ans)]
        }
        "dns_late" => {
            let ans = response(
                LATE.host,
                &[Answer::Addr(LATE.host, ip("142.250.200.2"), 300)],
                ResponseCode::NoError,
            );
            vec![dns_answer(
                SocketAddr::new(addr(LATE.client).ip(), 5353),
                &ans,
            )]
        }
        "wsl_truncated" => wsl_truncated(),
        "fasttrack_syn_only" => fasttrack_syn_only(),
        other => panic!("unknown capture scenario {other}"),
    }
}

/// Scenarios written as pcaps by `gen_capture_fixtures`.
pub const ALL: &[&str] = &[
    "tls_split_pq",
    "tls_ech",
    "quic_v1_rfc",
    "quic_v1_reordered",
    "quic_v2_rfc",
    "http_host",
    "shared_cdn",
    "dns_cname",
    "dns_late",
    "wsl_truncated",
    "fasttrack_syn_only",
];

/// Connections whose ClientHello needs two segments, but only the first
/// arrives: what a path dropping datagrams over 1472 bytes produces (US3).
pub const WSL_CONNECTIONS: u16 = 60;
/// TCP connections with a SYN but no captured payload (FastTrack, US3).
pub const FASTTRACK_SYNS: u16 = 120;

fn wsl_truncated() -> Vec<Vec<u8>> {
    let server = addr("142.250.1.1:443");
    (0..WSL_CONNECTIONS)
        .flat_map(|i| {
            let client = SocketAddr::new(ip("192.168.88.20"), 30_000 + i);
            let hello = tls_hello("www.google.com", &["h2"], true, false);
            // Full-size segments would exceed 1472 bytes once wrapped; only
            // the smaller first segments survive.
            tcp_stream(client, server, 10_000, &hello, 1_200)
                .into_iter()
                .take(2)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn fasttrack_syn_only() -> Vec<Vec<u8>> {
    let server = addr("142.250.1.1:443");
    (0..FASTTRACK_SYNS)
        .map(|i| {
            super::tcp(
                SocketAddr::new(ip("192.168.88.21"), 31_000 + i),
                server,
                1,
                SYN,
                &[],
            )
        })
        .collect()
}
