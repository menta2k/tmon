//! TZSP datagram → IP endpoints + TCP/UDP payload (R3, FR-003).

use etherparse::{NetSlice, SlicedPacket, TransportSlice};
use std::net::IpAddr;

use crate::tzsp::{self, TzspError};

pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

/// Why a datagram yields no packet (each maps to one counter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    MalformedTzsp,
    UnsupportedTzsp,
    UnsupportedEncap,
    /// Not IPv4/IPv6, or IP without TCP/UDP.
    UnsupportedL3,
    /// IPv4/IPv6 fragment (setup messages are never IP-fragmented).
    Fragment,
    /// Truncated or invalid Ethernet/IP/TCP/UDP header.
    MalformedFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpInfo {
    pub seq: u32,
    pub syn: bool,
    pub ack: bool,
    pub fin: bool,
    pub rst: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Tcp(TcpInfo),
    Udp,
}

/// One decoded packet; `payload` borrows the datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet<'a> {
    pub src_ip: IpAddr,
    pub dst_ip: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub transport: Transport,
    pub payload: &'a [u8],
    /// The whole IP packet (header + payload), for the classifier tap.
    pub l3: &'a [u8],
}

impl Packet<'_> {
    pub fn proto(&self) -> u8 {
        match self.transport {
            Transport::Tcp(_) => PROTO_TCP,
            Transport::Udp => PROTO_UDP,
        }
    }
}

const IPV6_HEADER_LEN: usize = 40;

/// The IP packet inside `frame` whose header is `header` (a subslice of
/// `frame`), `total` bytes long as stated by the header, clamped to the frame
/// (Ethernet padding beyond `total` is excluded).
fn l3_slice<'a>(frame: &'a [u8], header: &[u8], total: usize) -> &'a [u8] {
    let start = (header.as_ptr() as usize).saturating_sub(frame.as_ptr() as usize);
    let start = start.min(frame.len());
    let end = start
        .saturating_add(total.max(header.len()))
        .min(frame.len());
    &frame[start..end]
}

/// Decode one capture datagram (TZSP + Ethernet, ≤ 2 VLAN tags).
pub fn decode(datagram: &[u8]) -> Result<Packet<'_>, Reject> {
    let frame = tzsp::decode(datagram).map_err(|e| match e {
        TzspError::Malformed => Reject::MalformedTzsp,
        TzspError::Unsupported => Reject::UnsupportedTzsp,
        TzspError::Encap => Reject::UnsupportedEncap,
    })?;
    let p = SlicedPacket::from_ethernet(frame).map_err(|_| Reject::MalformedFrame)?;
    let (src_ip, dst_ip, l3) = match &p.net {
        Some(NetSlice::Ipv4(v4)) => (
            IpAddr::V4(v4.header().source_addr()),
            IpAddr::V4(v4.header().destination_addr()),
            l3_slice(
                frame,
                v4.header().slice(),
                usize::from(v4.header().total_len()),
            ),
        ),
        Some(NetSlice::Ipv6(v6)) => (
            IpAddr::V6(v6.header().source_addr()),
            IpAddr::V6(v6.header().destination_addr()),
            l3_slice(
                frame,
                v6.header().slice(),
                IPV6_HEADER_LEN + usize::from(v6.header().payload_length()),
            ),
        ),
        _ => return Err(Reject::UnsupportedL3),
    };
    if p.is_ip_payload_fragmented() {
        return Err(Reject::Fragment);
    }
    match p.transport {
        Some(TransportSlice::Tcp(t)) => Ok(Packet {
            src_ip,
            dst_ip,
            src_port: t.source_port(),
            dst_port: t.destination_port(),
            l3,
            transport: Transport::Tcp(TcpInfo {
                seq: t.sequence_number(),
                syn: t.syn(),
                ack: t.ack(),
                fin: t.fin(),
                rst: t.rst(),
            }),
            payload: t.payload(),
        }),
        Some(TransportSlice::Udp(u)) => Ok(Packet {
            src_ip,
            dst_ip,
            src_port: u.source_port(),
            dst_port: u.destination_port(),
            transport: Transport::Udp,
            payload: u.payload(),
            l3,
        }),
        _ => Err(Reject::UnsupportedL3),
    }
}
