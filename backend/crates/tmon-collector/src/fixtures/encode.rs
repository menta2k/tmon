//! Encoders for NetFlow v5, NetFlow v9, IPFIX and sFlow v5 datagrams.

use std::net::IpAddr;

/// One synthetic flow to encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowSpec {
    pub start_ms: i64,
    pub end_ms: i64,
    pub src: IpAddr,
    pub dst: IpAddr,
    pub sport: u16,
    pub dport: u16,
    pub proto: u8,
    pub tcp_flags: u8,
    pub bytes: u64,
    pub packets: u64,
    pub in_if: u32,
    pub out_if: u32,
    /// Post-NAT source; `None` = untranslated (RouterOS then repeats the original).
    pub nat: Option<(IpAddr, u16)>,
    /// Post-NAT destination; `None` = untranslated.
    pub nat_dst: Option<(IpAddr, u16)>,
}

const UPTIME_MS: u32 = 100_000_000;
pub const V9_TEMPLATE_V4: u16 = 256;
pub const V9_TEMPLATE_V6: u16 = 257;

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_be_bytes());
}
fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_be_bytes());
}
fn put64(b: &mut Vec<u8>, v: u64) {
    b.extend_from_slice(&v.to_be_bytes());
}
fn put_ip(b: &mut Vec<u8>, ip: IpAddr) {
    match ip {
        IpAddr::V4(a) => b.extend_from_slice(&a.octets()),
        IpAddr::V6(a) => b.extend_from_slice(&a.octets()),
    }
}
fn secs(ms: i64) -> u32 {
    u32::try_from(ms.div_euclid(1000)).unwrap_or(0)
}
/// sysUpTime (ms) at which an absolute timestamp happened, given export time.
fn uptime_at(export_ms: i64, at_ms: i64) -> u32 {
    let delta = u32::try_from((export_ms - at_ms).max(0)).unwrap_or(u32::MAX);
    UPTIME_MS.saturating_sub(delta)
}

/// NetFlow v5 datagram (IPv4 only, ≤ 30 flows).
pub fn encode_v5(export_ms: i64, seq: u32, sampling: u16, flows: &[FlowSpec]) -> Vec<u8> {
    let mut b = Vec::with_capacity(24 + flows.len() * 48);
    put16(&mut b, 5);
    put16(&mut b, flows.len() as u16);
    put32(&mut b, UPTIME_MS);
    put32(&mut b, secs(export_ms));
    put32(&mut b, ((export_ms.rem_euclid(1000)) * 1_000_000) as u32);
    put32(&mut b, seq);
    b.extend_from_slice(&[0, 0]);
    put16(&mut b, sampling);
    for f in flows {
        put_ip(&mut b, f.src);
        put_ip(&mut b, f.dst);
        put32(&mut b, 0);
        put16(&mut b, f.in_if as u16);
        put16(&mut b, f.out_if as u16);
        put32(&mut b, f.packets as u32);
        put32(&mut b, f.bytes as u32);
        put32(&mut b, uptime_at(export_ms, f.start_ms));
        put32(&mut b, uptime_at(export_ms, f.end_ms));
        put16(&mut b, f.sport);
        put16(&mut b, f.dport);
        b.extend_from_slice(&[0, f.tcp_flags, f.proto, 0]);
        b.extend_from_slice(&[0; 8]);
    }
    b
}

/// (field type, length) lists for the v9 templates.
fn v9_fields(v6: bool) -> Vec<(u16, u16)> {
    let (src, dst, alen) = if v6 { (27, 28, 16) } else { (8, 12, 4) };
    vec![
        (src, alen),
        (dst, alen),
        (7, 2),
        (11, 2),
        (4, 1),
        (6, 1),
        (1, 8),
        (2, 8),
        (10, 4),
        (14, 4),
        (22, 4),
        (21, 4),
    ]
}

/// NetFlow v9 datagram. `with_template` controls whether templates are sent.
pub fn encode_v9(
    export_ms: i64,
    seq: u32,
    source_id: u32,
    with_template: bool,
    flows: &[FlowSpec],
) -> Vec<u8> {
    let mut b = Vec::new();
    put16(&mut b, 9);
    let count_at = b.len();
    put16(&mut b, 0);
    put32(&mut b, UPTIME_MS);
    put32(&mut b, secs(export_ms));
    put32(&mut b, seq);
    put32(&mut b, source_id);
    let mut count = 0u16;
    if with_template {
        let mut set = Vec::new();
        for (id, v6) in [(V9_TEMPLATE_V4, false), (V9_TEMPLATE_V6, true)] {
            let fields = v9_fields(v6);
            put16(&mut set, id);
            put16(&mut set, fields.len() as u16);
            fields.iter().for_each(|(t, l)| {
                put16(&mut set, *t);
                put16(&mut set, *l);
            });
            count += 1;
        }
        push_set(&mut b, 0, &set);
    }
    for (id, v6) in [(V9_TEMPLATE_V4, false), (V9_TEMPLATE_V6, true)] {
        let mut set = Vec::new();
        for f in flows.iter().filter(|f| f.src.is_ipv6() == v6) {
            put_ip(&mut set, f.src);
            put_ip(&mut set, f.dst);
            put16(&mut set, f.sport);
            put16(&mut set, f.dport);
            set.extend_from_slice(&[f.proto, f.tcp_flags]);
            put64(&mut set, f.bytes);
            put64(&mut set, f.packets);
            put32(&mut set, f.in_if);
            put32(&mut set, f.out_if);
            put32(&mut set, uptime_at(export_ms, f.start_ms));
            put32(&mut set, uptime_at(export_ms, f.end_ms));
            count += 1;
        }
        if !set.is_empty() {
            push_set(&mut b, id, &set);
        }
    }
    b[count_at..count_at + 2].copy_from_slice(&count.to_be_bytes());
    b
}

/// Append a flowset/set with header and 4-byte padding.
fn push_set(b: &mut Vec<u8>, id: u16, body: &[u8]) {
    let pad = (4 - (4 + body.len()) % 4) % 4;
    put16(b, id);
    put16(b, (4 + body.len() + pad) as u16);
    b.extend_from_slice(body);
    b.extend(std::iter::repeat_n(0u8, pad));
}

fn ipfix_fields(v6: bool) -> Vec<(u16, u16)> {
    let (src, dst, alen) = if v6 { (27, 28, 16) } else { (8, 12, 4) };
    let mut f = vec![
        (src, alen),
        (dst, alen),
        (7, 2),
        (11, 2),
        (4, 1),
        (6, 1),
        (1, 8),
        (2, 8),
        (10, 4),
        (14, 4),
        (152, 8),
        (153, 8),
    ];
    if !v6 {
        f.extend_from_slice(&[(225, 4), (226, 4), (227, 2), (228, 2)]);
    }
    f
}

/// IPFIX message as exported by RouterOS v7 (`version=ipfix`).
pub fn encode_ipfix(
    export_ms: i64,
    seq: u32,
    domain: u32,
    with_template: bool,
    flows: &[FlowSpec],
) -> Vec<u8> {
    let mut b = Vec::new();
    put16(&mut b, 10);
    put16(&mut b, 0);
    put32(&mut b, secs(export_ms));
    put32(&mut b, seq);
    put32(&mut b, domain);
    if with_template {
        let mut set = Vec::new();
        for (id, v6) in [(V9_TEMPLATE_V4, false), (V9_TEMPLATE_V6, true)] {
            let fields = ipfix_fields(v6);
            put16(&mut set, id);
            put16(&mut set, fields.len() as u16);
            fields.iter().for_each(|(t, l)| {
                put16(&mut set, *t);
                put16(&mut set, *l);
            });
        }
        push_set(&mut b, 2, &set);
    }
    for (id, v6) in [(V9_TEMPLATE_V4, false), (V9_TEMPLATE_V6, true)] {
        let mut set = Vec::new();
        for f in flows.iter().filter(|f| f.src.is_ipv6() == v6) {
            ipfix_record(&mut set, f, v6);
        }
        if !set.is_empty() {
            push_set(&mut b, id, &set);
        }
    }
    let len = b.len() as u16;
    b[2..4].copy_from_slice(&len.to_be_bytes());
    b
}

fn ipfix_record(set: &mut Vec<u8>, f: &FlowSpec, v6: bool) {
    put_ip(set, f.src);
    put_ip(set, f.dst);
    put16(set, f.sport);
    put16(set, f.dport);
    set.extend_from_slice(&[f.proto, f.tcp_flags]);
    put64(set, f.bytes);
    put64(set, f.packets);
    put32(set, f.in_if);
    put32(set, f.out_if);
    put64(set, f.start_ms as u64);
    put64(set, f.end_ms as u64);
    if !v6 {
        // Like RouterOS: untranslated endpoints repeat the original address/port.
        let (sip, sport) = f.nat.unwrap_or((f.src, f.sport));
        let (dip, dport) = f.nat_dst.unwrap_or((f.dst, f.dport));
        put_ip(set, sip);
        put_ip(set, dip);
        put16(set, sport);
        put16(set, dport);
    }
}

/// sFlow v5 datagram with one flow sample (sampled Ethernet header) per flow.
/// Each `FlowSpec` represents ONE sampled packet of `bytes` frame length.
pub fn encode_sflow(agent: IpAddr, seq: u32, sampling_rate: u32, flows: &[FlowSpec]) -> Vec<u8> {
    let mut b = Vec::new();
    put32(&mut b, 5);
    put32(&mut b, if agent.is_ipv4() { 1 } else { 2 });
    put_ip(&mut b, agent);
    put32(&mut b, 0);
    put32(&mut b, seq);
    put32(&mut b, UPTIME_MS);
    put32(&mut b, flows.len() as u32);
    for (i, f) in flows.iter().enumerate() {
        let sample = flow_sample(seq.wrapping_mul(100) + i as u32, sampling_rate, f);
        put32(&mut b, 1);
        put32(&mut b, sample.len() as u32);
        b.extend_from_slice(&sample);
    }
    b
}

fn flow_sample(seq: u32, rate: u32, f: &FlowSpec) -> Vec<u8> {
    let header = packet_header(f);
    let mut rec = Vec::new();
    put32(&mut rec, 1);
    put32(&mut rec, f.bytes as u32);
    put32(&mut rec, 4);
    put32(&mut rec, header.len() as u32);
    rec.extend_from_slice(&header);
    rec.extend(std::iter::repeat_n(0u8, (4 - header.len() % 4) % 4));
    let mut s = Vec::new();
    put32(&mut s, seq);
    put32(&mut s, f.in_if);
    put32(&mut s, rate);
    put32(&mut s, seq.wrapping_mul(rate));
    put32(&mut s, 0);
    put32(&mut s, f.in_if);
    put32(&mut s, f.out_if);
    put32(&mut s, 1);
    put32(&mut s, 1);
    put32(&mut s, rec.len() as u32);
    s.extend_from_slice(&rec);
    s
}

/// Ethernet + IP + TCP/UDP header bytes for a sampled packet.
fn packet_header(f: &FlowSpec) -> Vec<u8> {
    let mut h = vec![0x02, 0, 0, 0, 0, 1, 0x02, 0, 0, 0, 0, 2];
    let l4 = l4_header(f);
    match (f.src, f.dst) {
        (IpAddr::V4(s), IpAddr::V4(d)) => {
            put16(&mut h, 0x0800);
            h.extend_from_slice(&[0x45, 0]);
            put16(&mut h, (20 + l4.len()) as u16);
            h.extend_from_slice(&[0, 0, 0x40, 0, 64, f.proto, 0, 0]);
            h.extend_from_slice(&s.octets());
            h.extend_from_slice(&d.octets());
        }
        (s, d) => {
            put16(&mut h, 0x86dd);
            h.extend_from_slice(&[0x60, 0, 0, 0]);
            put16(&mut h, l4.len() as u16);
            h.extend_from_slice(&[f.proto, 64]);
            put_ip(&mut h, s);
            put_ip(&mut h, d);
        }
    }
    h.extend_from_slice(&l4);
    h
}

fn l4_header(f: &FlowSpec) -> Vec<u8> {
    let mut l4 = Vec::new();
    put16(&mut l4, f.sport);
    put16(&mut l4, f.dport);
    if f.proto == 6 {
        l4.extend_from_slice(&[0; 8]);
        l4.extend_from_slice(&[0x50, f.tcp_flags]);
        l4.extend_from_slice(&[0; 6]);
    } else {
        l4.extend_from_slice(&[0, 8, 0, 0]);
    }
    l4
}
