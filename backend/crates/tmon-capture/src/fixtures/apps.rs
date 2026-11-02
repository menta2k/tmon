//! Application fixtures for the classifier (feature 003, research R12):
//! connection setups of protocols without a server name, built from their
//! public specifications, in both directions.

use std::net::SocketAddr;

use super::hello::{Hello, client_hello, record};
use super::{ACK, Flags, SYN, addr, tcp, tcp_stream, udp};

/// One application connection and what it should become.
#[derive(Debug, Clone, Copy)]
pub struct AppExpect {
    pub scenario: &'static str,
    pub client: &'static str,
    pub server: &'static str,
    pub proto: u8,
    /// nDPI application name.
    pub app: &'static str,
    /// nDPI category name.
    pub ndpi_category: &'static str,
    /// tmon category after the built-in app rules.
    pub category: &'static str,
}

const SYN_ACK: Flags = Flags {
    syn: true,
    ack: true,
};

pub const APPS: &[AppExpect] = &[
    AppExpect {
        scenario: "bittorrent",
        client: "192.168.88.30:51001",
        server: "198.51.100.20:6881",
        proto: 6,
        app: "BitTorrent",
        ndpi_category: "Download",
        category: "File Sharing",
    },
    AppExpect {
        scenario: "wireguard",
        client: "192.168.88.31:51002",
        server: "198.51.100.21:51820",
        proto: 17,
        app: "WireGuard",
        ndpi_category: "VPN",
        category: "VPN/Tunnels",
    },
    AppExpect {
        scenario: "openvpn",
        client: "192.168.88.32:51003",
        server: "198.51.100.22:1194",
        proto: 17,
        app: "OpenVPN",
        ndpi_category: "VPN",
        category: "VPN/Tunnels",
    },
    AppExpect {
        scenario: "stun_rtp",
        client: "192.168.88.33:51004",
        server: "198.51.100.23:3478",
        proto: 17,
        app: "STUN",
        ndpi_category: "Network",
        category: "Messaging/VoIP",
    },
    AppExpect {
        scenario: "steam_a2s",
        client: "192.168.88.34:51005",
        server: "198.51.100.24:27015",
        proto: 17,
        app: "Source_Engine",
        ndpi_category: "Game",
        category: "Gaming",
    },
    AppExpect {
        scenario: "rdp",
        client: "192.168.88.35:51006",
        server: "198.51.100.25:3389",
        proto: 6,
        app: "RDP",
        ndpi_category: "RemoteAccess",
        category: "Remote Access",
    },
];

/// TLS to `zoom.us` (named by feature 002, also recognized by nDPI).
pub const ZOOM: AppExpect = AppExpect {
    scenario: "zoom_tls",
    client: "192.168.88.36:51007",
    server: "170.114.52.2:443",
    proto: 6,
    app: "Zoom",
    ndpi_category: "Video",
    category: "Messaging/VoIP",
};

/// UDP to the WireGuard port with a payload that is not WireGuard.
pub const PORT_ONLY: AppExpect = AppExpect {
    scenario: "port_only",
    client: "192.168.88.37:51008",
    server: "198.51.100.27:51820",
    proto: 17,
    app: "WireGuard",
    ndpi_category: "VPN",
    category: "VPN/Tunnels",
};

fn ends(e: &AppExpect) -> (SocketAddr, SocketAddr) {
    (addr(e.client), addr(e.server))
}

/// TCP three-way handshake, then client and server payloads.
fn tcp_exchange(c: SocketAddr, s: SocketAddr, req: &[u8], resp: &[u8]) -> Vec<Vec<u8>> {
    vec![
        tcp(c, s, 1_000, SYN, &[]),
        tcp(s, c, 9_000, SYN_ACK, &[]),
        tcp(c, s, 1_001, ACK, req),
        tcp(s, c, 9_001, ACK, resp),
    ]
}

/// BEP 3 handshake: pstrlen, "BitTorrent protocol", reserved, info hash, peer id.
fn bittorrent_handshake(peer: u8) -> Vec<u8> {
    let mut h = vec![19];
    h.extend_from_slice(b"BitTorrent protocol");
    h.extend_from_slice(&[0, 0, 0, 0, 0, 0x10, 0, 0x05]);
    h.extend_from_slice(&[0xab; 20]);
    h.extend_from_slice(b"-qB4650-");
    h.extend_from_slice(&[peer; 12]);
    h
}

/// WireGuard handshake initiation (148 B) and response (92 B).
fn wireguard() -> (Vec<u8>, Vec<u8>) {
    let sender = [0x11, 0x22, 0x33, 0x44];
    let mut init = vec![1, 0, 0, 0];
    init.extend_from_slice(&sender);
    init.resize(148, 0x5c);
    let mut resp = vec![2, 0, 0, 0, 0x55, 0x66, 0x77, 0x88];
    resp.extend_from_slice(&sender);
    resp.resize(92, 0x3d);
    (init, resp)
}

/// OpenVPN (tls-crypt, no HMAC) hard resets: client v2 and server v2 acking it.
fn openvpn() -> (Vec<u8>, Vec<u8>) {
    let client_sid = [0xc1, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
    let server_sid = [0x5e, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33];
    let mut c = vec![0x07 << 3];
    c.extend_from_slice(&client_sid);
    c.extend_from_slice(&[0, 0, 0, 0, 0]);
    let mut s = vec![0x08 << 3];
    s.extend_from_slice(&server_sid);
    s.extend_from_slice(&[1, 0, 0, 0, 0]);
    s.extend_from_slice(&client_sid);
    s.extend_from_slice(&[0, 0, 0, 0]);
    (c, s)
}

const STUN_COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];

/// RFC 8489 binding request and success response (XOR-MAPPED-ADDRESS).
fn stun() -> (Vec<u8>, Vec<u8>) {
    let txid = [7u8; 12];
    let mut req = vec![0x00, 0x01, 0x00, 0x00];
    req.extend_from_slice(&STUN_COOKIE);
    req.extend_from_slice(&txid);
    let mut resp = vec![0x01, 0x01, 0x00, 0x0c];
    resp.extend_from_slice(&STUN_COOKIE);
    resp.extend_from_slice(&txid);
    resp.extend_from_slice(&[0x00, 0x20, 0x00, 0x08, 0x00, 0x01, 0xe0, 0x16]);
    resp.extend_from_slice(&[0xe1, 0xba, 0xa5, 0x2b]);
    (req, resp)
}

/// RTP (PT 111, Opus) with sequence `seq`.
fn rtp(seq: u16, ssrc: u32) -> Vec<u8> {
    let mut p = vec![0x80, 111];
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(&(u32::from(seq) * 960).to_be_bytes());
    p.extend_from_slice(&ssrc.to_be_bytes());
    p.extend_from_slice(&[0x78; 60]);
    p
}

/// A2S_INFO query and info response (Valve server queries).
fn a2s() -> (Vec<u8>, Vec<u8>) {
    let mut q = vec![0xff, 0xff, 0xff, 0xff];
    q.extend_from_slice(b"TSource Engine Query\0");
    let mut r = vec![0xff, 0xff, 0xff, 0xff, b'I', 0x11];
    r.extend_from_slice(b"tmon test server\0de_dust2\0csgo\0Counter-Strike\0");
    r.extend_from_slice(&[0xda, 0x02, 3, 10, 0, b'd', b'l', 0, 1]);
    r.extend_from_slice(b"1.38.7.9\0");
    (q, r)
}

/// X.224 Connection Request / Confirm with RDP negotiation (MS-RDPBCGR 2.2.1.1/2.2.1.2).
fn rdp() -> (Vec<u8>, Vec<u8>) {
    let req = vec![
        0x03, 0x00, 0x00, 0x13, 0x0e, 0xe0, 0, 0, 0, 0, 0, 0x01, 0x00, 0x08, 0x00, 0, 0, 0, 0,
    ];
    let resp = vec![
        0x03, 0x00, 0x00, 0x13, 0x0e, 0xd0, 0, 0, 0x12, 0x34, 0, 0x02, 0x00, 0x08, 0x00, 0, 0, 0, 0,
    ];
    (req, resp)
}

fn zoom() -> Vec<Vec<u8>> {
    let (c, s) = ends(&ZOOM);
    let hello = record(&client_hello(&Hello {
        sni: Some("zoom.us"),
        alpn: &["http/1.1"],
        ..Hello::default()
    }));
    tcp_stream(c, s, 5_000, &hello, 1_400)
}

/// The TZSP datagrams of one application scenario, in send order.
pub fn datagrams(e: &AppExpect) -> Vec<Vec<u8>> {
    let (c, s) = ends(e);
    match e.scenario {
        "bittorrent" => tcp_exchange(c, s, &bittorrent_handshake(1), &bittorrent_handshake(2)),
        "wireguard" => {
            let (i, r) = wireguard();
            vec![udp(c, s, &i), udp(s, c, &r)]
        }
        "openvpn" => {
            let (a, b) = openvpn();
            vec![udp(c, s, &a), udp(s, c, &b)]
        }
        "stun_rtp" => {
            let (a, b) = stun();
            let mut v = vec![udp(c, s, &a), udp(s, c, &b)];
            for seq in 1..=4u16 {
                v.push(udp(c, s, &rtp(seq, 0x1234_5678)));
                v.push(udp(s, c, &rtp(seq, 0x0bad_cafe)));
            }
            v
        }
        "steam_a2s" => {
            let (q, r) = a2s();
            vec![udp(c, s, &q), udp(s, c, &r)]
        }
        "rdp" => {
            let (a, b) = rdp();
            tcp_exchange(c, s, &a, &b)
        }
        "zoom_tls" => zoom(),
        "port_only" => (0..6u8).map(|i| udp(c, s, &[0x9e ^ i; 64])).collect(),
        other => panic!("unknown app scenario {other}"),
    }
}

/// Every connection of [`APPS`], interleaved per scenario.
pub fn mix() -> Vec<Vec<u8>> {
    APPS.iter().flat_map(datagrams).collect()
}
