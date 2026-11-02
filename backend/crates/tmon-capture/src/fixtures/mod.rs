//! Packet builders for tests, benches, fuzz seeds and `gen_capture_fixtures`
//! (feature `fixtures`). Everything returns TZSP datagrams as RouterOS sends them.

pub mod apps;
pub mod dns;
pub mod hello;
pub mod quic;
pub mod scenarios;

use etherparse::PacketBuilder;
use std::net::{IpAddr, SocketAddr};

use crate::tzsp;

const SRC_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x01];
const DST_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x02];
const ETHERTYPE_VLAN: u16 = 0x8100;
const ETHERTYPE_QINQ: u16 = 0x88a8;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERTYPE_IPV6: u16 = 0x86dd;
const TTL: u8 = 64;
const WINDOW: u16 = 64_240;

/// TCP flags for [`tcp`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub syn: bool,
    pub ack: bool,
}

pub const SYN: Flags = Flags {
    syn: true,
    ack: false,
};
pub const ACK: Flags = Flags {
    syn: false,
    ack: true,
};

/// IP packet (no link layer) carrying TCP.
pub fn tcp_ip(src: SocketAddr, dst: SocketAddr, seq: u32, flags: Flags, payload: &[u8]) -> Vec<u8> {
    let b = ip_builder(src.ip(), dst.ip()).tcp(src.port(), dst.port(), seq, WINDOW);
    let b = if flags.syn { b.syn() } else { b };
    let b = if flags.ack { b.ack(1) } else { b };
    let mut out = Vec::with_capacity(b.size(payload.len()));
    b.write(&mut out, payload).expect("in-memory write");
    out
}

/// IP packet (no link layer) carrying UDP.
pub fn udp_ip(src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let b = ip_builder(src.ip(), dst.ip()).udp(src.port(), dst.port());
    let mut out = Vec::with_capacity(b.size(payload.len()));
    b.write(&mut out, payload).expect("in-memory write");
    out
}

fn ip_builder(src: IpAddr, dst: IpAddr) -> etherparse::PacketBuilderStep<etherparse::IpHeaders> {
    match (src, dst) {
        (IpAddr::V4(s), IpAddr::V4(d)) => PacketBuilder::ipv4(s.octets(), d.octets(), TTL),
        (IpAddr::V6(s), IpAddr::V6(d)) => PacketBuilder::ipv6(s.octets(), d.octets(), TTL),
        _ => panic!("mixed address families"),
    }
}

/// Ethernet II frame around an IP packet, with 0–2 VLAN tags.
pub fn ethernet(ip_packet: &[u8], vlans: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ip_packet.len() + 14 + 4 * vlans.len());
    out.extend_from_slice(&DST_MAC);
    out.extend_from_slice(&SRC_MAC);
    for (i, vid) in vlans.iter().enumerate() {
        let tpid = if i == 0 && vlans.len() == 2 {
            ETHERTYPE_QINQ
        } else {
            ETHERTYPE_VLAN
        };
        out.extend_from_slice(&tpid.to_be_bytes());
        out.extend_from_slice(&(vid & 0x0fff).to_be_bytes());
    }
    let ethertype = if ip_packet.first().is_some_and(|b| b >> 4 == 6) {
        ETHERTYPE_IPV6
    } else {
        ETHERTYPE_IPV4
    };
    out.extend_from_slice(&ethertype.to_be_bytes());
    out.extend_from_slice(ip_packet);
    out
}

/// TZSP datagram around an IP packet (no VLAN).
pub fn tzsp_ip(ip_packet: &[u8]) -> Vec<u8> {
    tzsp::encode(&ethernet(ip_packet, &[]))
}

/// TZSP datagram for one TCP segment.
pub fn tcp(src: SocketAddr, dst: SocketAddr, seq: u32, flags: Flags, payload: &[u8]) -> Vec<u8> {
    tzsp_ip(&tcp_ip(src, dst, seq, flags, payload))
}

/// TZSP datagram for one UDP datagram.
pub fn udp(src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> Vec<u8> {
    tzsp_ip(&udp_ip(src, dst, payload))
}

/// A client TCP connection: SYN, then the payload cut into `mss`-sized segments.
pub fn tcp_stream(
    client: SocketAddr,
    server: SocketAddr,
    isn: u32,
    payload: &[u8],
    mss: usize,
) -> Vec<Vec<u8>> {
    let mut out = vec![tcp(client, server, isn, SYN, &[])];
    let mut seq = isn.wrapping_add(1);
    for chunk in payload.chunks(mss.max(1)) {
        out.push(tcp(client, server, seq, ACK, chunk));
        seq = seq.wrapping_add(chunk.len() as u32);
    }
    out
}

pub fn addr(s: &str) -> SocketAddr {
    s.parse().expect("fixture address")
}
