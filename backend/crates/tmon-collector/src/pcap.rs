//! Minimal classic-pcap reader/writer for UDP datagrams (replay + fixtures).
//!
//! Supports link types Ethernet (1, incl. one 802.1Q tag), raw IP (101, 228/229)
//! and Linux cooked capture (113). Only UDP over IPv4/IPv6 is extracted.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const MAGIC_US: u32 = 0xa1b2_c3d4;
const MAGIC_NS: u32 = 0xa1b2_3c4d;
const LINKTYPE_ETHERNET: u32 = 1;
const MAX_SNAPLEN: u32 = 262_144;

/// One captured UDP datagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UdpPacket {
    pub ts_us: i64,
    pub src: SocketAddr,
    pub dst: SocketAddr,
    pub payload: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum PcapError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("not a pcap file (bad magic {0:#x})")]
    BadMagic(u32),
    #[error("unsupported link type {0}")]
    LinkType(u32),
    #[error("corrupt record: {0}")]
    Corrupt(&'static str),
}

/// Read all UDP datagrams from a pcap stream (non-UDP frames are skipped).
pub fn read_udp<R: Read>(mut r: R) -> Result<Vec<UdpPacket>, PcapError> {
    let mut hdr = [0u8; 24];
    r.read_exact(&mut hdr)?;
    let magic_le = u32::from_le_bytes(hdr[0..4].try_into().expect("4 bytes"));
    let (le, nanos) = match magic_le {
        MAGIC_US => (true, false),
        MAGIC_NS => (true, true),
        m if m.swap_bytes() == MAGIC_US => (false, false),
        m if m.swap_bytes() == MAGIC_NS => (false, true),
        m => return Err(PcapError::BadMagic(m)),
    };
    let rd = |b: &[u8]| {
        let a: [u8; 4] = b.try_into().expect("4 bytes");
        if le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        }
    };
    let linktype = rd(&hdr[20..24]);
    let mut out = Vec::new();
    let mut rec = [0u8; 16];
    loop {
        match r.read_exact(&mut rec) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let (sec, frac, incl) = (rd(&rec[0..4]), rd(&rec[4..8]), rd(&rec[8..12]));
        if incl > MAX_SNAPLEN {
            return Err(PcapError::Corrupt("record length exceeds snaplen"));
        }
        let mut frame = vec![0u8; incl as usize];
        r.read_exact(&mut frame)?;
        let us = if nanos { frac / 1000 } else { frac };
        let ts_us = i64::from(sec) * 1_000_000 + i64::from(us);
        if let Some(p) = parse_frame(linktype, &frame)? {
            out.push(UdpPacket { ts_us, ..p });
        }
    }
    Ok(out)
}

fn parse_frame(linktype: u32, f: &[u8]) -> Result<Option<UdpPacket>, PcapError> {
    let ip = match linktype {
        LINKTYPE_ETHERNET => ethernet_payload(f),
        101 | 228 | 229 => Some(f),
        113 => f
            .get(16..)
            .map(|p| (be16(f, 14), p))
            .and_then(|(et, p)| ip_by_ethertype(et, p)),
        other => return Err(PcapError::LinkType(other)),
    };
    Ok(ip.and_then(parse_ip_udp))
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn ethernet_payload(f: &[u8]) -> Option<&[u8]> {
    if f.len() < 14 {
        return None;
    }
    let mut et = be16(f, 12);
    let mut off = 14;
    if et == 0x8100 && f.len() >= 18 {
        et = be16(f, 16);
        off = 18;
    }
    ip_by_ethertype(et, &f[off..])
}

fn ip_by_ethertype(et: u16, p: &[u8]) -> Option<&[u8]> {
    matches!(et, 0x0800 | 0x86dd).then_some(p)
}

fn parse_ip_udp(p: &[u8]) -> Option<UdpPacket> {
    let version = p.first()? >> 4;
    let (src, dst, l4) = match version {
        4 => {
            let ihl = usize::from(p[0] & 0x0f) * 4;
            if p.len() < ihl.max(20) || p[9] != 17 {
                return None;
            }
            let s = Ipv4Addr::new(p[12], p[13], p[14], p[15]);
            let d = Ipv4Addr::new(p[16], p[17], p[18], p[19]);
            (IpAddr::V4(s), IpAddr::V4(d), &p[ihl..])
        }
        6 => {
            if p.len() < 40 || p[6] != 17 {
                return None;
            }
            let s: [u8; 16] = p[8..24].try_into().ok()?;
            let d: [u8; 16] = p[24..40].try_into().ok()?;
            (
                IpAddr::V6(Ipv6Addr::from(s)),
                IpAddr::V6(Ipv6Addr::from(d)),
                &p[40..],
            )
        }
        _ => return None,
    };
    if l4.len() < 8 {
        return None;
    }
    let ulen = usize::from(be16(l4, 4)).clamp(8, l4.len());
    Some(UdpPacket {
        ts_us: 0,
        src: SocketAddr::new(src, be16(l4, 0)),
        dst: SocketAddr::new(dst, be16(l4, 2)),
        payload: l4[8..ulen].to_vec(),
    })
}

/// Write UDP datagrams as an Ethernet/IPv4 pcap (IPv4 endpoints only).
pub fn write_udp<W: Write>(mut w: W, packets: &[UdpPacket]) -> io::Result<()> {
    let mut hdr = Vec::with_capacity(24);
    hdr.extend_from_slice(&MAGIC_US.to_le_bytes());
    hdr.extend_from_slice(&2u16.to_le_bytes());
    hdr.extend_from_slice(&4u16.to_le_bytes());
    hdr.extend_from_slice(&[0u8; 8]);
    hdr.extend_from_slice(&65_535u32.to_le_bytes());
    hdr.extend_from_slice(&LINKTYPE_ETHERNET.to_le_bytes());
    w.write_all(&hdr)?;
    for p in packets {
        let frame = build_frame(p)?;
        let sec = u32::try_from(p.ts_us.div_euclid(1_000_000)).unwrap_or(0);
        let us = u32::try_from(p.ts_us.rem_euclid(1_000_000)).unwrap_or(0);
        let len = u32::try_from(frame.len()).map_err(io::Error::other)?;
        for v in [sec, us, len, len] {
            w.write_all(&v.to_le_bytes())?;
        }
        w.write_all(&frame)?;
    }
    Ok(())
}

fn build_frame(p: &UdpPacket) -> io::Result<Vec<u8>> {
    let (IpAddr::V4(s), IpAddr::V4(d)) = (p.src.ip(), p.dst.ip()) else {
        return Err(io::Error::other(
            "only IPv4 endpoints supported for writing",
        ));
    };
    let udp_len = u16::try_from(8 + p.payload.len()).map_err(io::Error::other)?;
    let ip_len = udp_len + 20;
    let mut f = Vec::with_capacity(14 + usize::from(ip_len));
    f.extend_from_slice(&[0x02, 0, 0, 0, 0, 1, 0x02, 0, 0, 0, 0, 2, 0x08, 0x00]);
    let mut ip = vec![0x45, 0];
    ip.extend_from_slice(&ip_len.to_be_bytes());
    ip.extend_from_slice(&[0, 0, 0x40, 0, 64, 17, 0, 0]);
    ip.extend_from_slice(&s.octets());
    ip.extend_from_slice(&d.octets());
    let csum = ipv4_checksum(&ip);
    ip[10..12].copy_from_slice(&csum.to_be_bytes());
    f.extend_from_slice(&ip);
    f.extend_from_slice(&p.src.port().to_be_bytes());
    f.extend_from_slice(&p.dst.port().to_be_bytes());
    f.extend_from_slice(&udp_len.to_be_bytes());
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&p.payload);
    Ok(f)
}

fn ipv4_checksum(h: &[u8]) -> u16 {
    let mut sum: u32 = h
        .chunks(2)
        .map(|c| u32::from(u16::from_be_bytes([c[0], c[1]])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let pkts = vec![UdpPacket {
            ts_us: 1_700_000_000_123_456,
            src: "192.168.88.1:40000".parse().unwrap(),
            dst: "10.0.0.5:2055".parse().unwrap(),
            payload: vec![1, 2, 3, 4, 5],
        }];
        let mut buf = Vec::new();
        write_udp(&mut buf, &pkts).unwrap();
        assert_eq!(read_udp(buf.as_slice()).unwrap(), pkts);
    }

    #[test]
    fn rejects_bad_magic() {
        let err = read_udp([0u8; 24].as_slice()).unwrap_err();
        assert!(matches!(err, PcapError::BadMagic(_)));
    }
}
