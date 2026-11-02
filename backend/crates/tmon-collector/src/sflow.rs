//! sFlow v5 decoding (FR-002): sampled packet headers → `RawFlow` scaled by
//! the sampling rate. Counter samples only register the agent.

use sflow_parser::models::core::{Address, FlowData, FlowRecord, SampleData};
use sflow_parser::models::record_flows::{HeaderProtocol, SampledHeader};
use sflow_parser::parse_datagram;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tmon_core::RawFlow;

use crate::exporter_registry::{Exporter, ExporterRegistry, FlowProtocol};
use crate::netflow::Decoded;

/// Stateless sFlow decoder.
#[derive(Debug, Default, Clone, Copy)]
pub struct SflowDecoder;

/// 5-tuple + flags parsed out of a sampled header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tuple {
    src: IpAddr,
    dst: IpAddr,
    sport: u16,
    dport: u16,
    proto: u8,
    flags: u8,
}

impl SflowDecoder {
    pub fn new() -> Self {
        Self
    }

    pub fn decode(
        &self,
        src: SocketAddr,
        data: &[u8],
        reg: &ExporterRegistry,
        now_ms: i64,
    ) -> Decoded {
        let dgram = match parse_datagram(data) {
            Ok(d) => d,
            Err(_) => {
                let Some((e, is_new)) =
                    reg.get_or_create(src.ip(), 0, FlowProtocol::Sflow5, now_ms)
                else {
                    return Decoded {
                        exporter_limit: true,
                        ..Decoded::default()
                    };
                };
                e.counters.add_received();
                e.counters.add_parse_error();
                return Decoded {
                    new_exporters: new_list(e, is_new),
                    ..Decoded::default()
                };
            }
        };
        let agent = match dgram.agent_address {
            Address::IPv4(a) => IpAddr::V4(a),
            Address::IPv6(a) => IpAddr::V6(a),
            Address::Unknown => src.ip(),
        };
        let Some((e, is_new)) =
            reg.get_or_create(agent, dgram.sub_agent_id, FlowProtocol::Sflow5, now_ms)
        else {
            return Decoded {
                exporter_limit: true,
                ..Decoded::default()
            };
        };
        e.counters.add_received();
        e.touch(FlowProtocol::Sflow5, now_ms);
        let mut flows = Vec::new();
        for sample in &dgram.samples {
            let (rate, input, output, records) = match &sample.sample_data {
                SampleData::FlowSample(s) => (
                    s.sampling_rate,
                    s.input.value(),
                    s.output.value(),
                    &s.flow_records,
                ),
                SampleData::FlowSampleExpanded(s) => (
                    s.sampling_rate,
                    s.input.value,
                    s.output.value,
                    &s.flow_records,
                ),
                _ => continue,
            };
            if rate > 1 {
                e.set_advertised_sampling(rate);
            }
            let rate = u64::from(e.sampling_rate());
            for rec in records {
                if let Some((t, len)) = tuple_of(rec) {
                    flows.push(raw(&e, t, len, rate, (input, output), now_ms));
                }
            }
        }
        let bytes = flows
            .iter()
            .fold(0u64, |acc, f| acc.saturating_add(f.bytes));
        e.counters.add_decoded(flows.len() as u64, bytes);
        Decoded {
            flows,
            new_exporters: new_list(e, is_new),
            ..Decoded::default()
        }
    }
}

fn new_list(e: Arc<Exporter>, is_new: bool) -> Vec<Arc<Exporter>> {
    if is_new { vec![e] } else { Vec::new() }
}

fn raw(e: &Exporter, t: Tuple, frame_len: u64, rate: u64, ifs: (u32, u32), now_ms: i64) -> RawFlow {
    RawFlow {
        ts_ms: now_ms,
        ts_start_ms: now_ms,
        exporter_id: e.id,
        in_if: ifs.0,
        out_if: ifs.1,
        src_ip: t.src,
        dst_ip: t.dst,
        src_port: t.sport,
        dst_port: t.dport,
        proto: t.proto,
        tcp_flags: t.flags,
        bytes: crate::fields::scaled(frame_len, rate),
        packets: rate,
        sampled: rate > 1,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    }
}

fn tuple_of(rec: &FlowRecord) -> Option<(Tuple, u64)> {
    match &rec.flow_data {
        FlowData::SampledHeader(h) => header_tuple(h).map(|t| (t, u64::from(h.frame_length))),
        FlowData::SampledIpv4(s) => Some((
            Tuple {
                src: IpAddr::V4(s.src_ip),
                dst: IpAddr::V4(s.dst_ip),
                sport: s.src_port as u16,
                dport: s.dst_port as u16,
                proto: s.protocol as u8,
                flags: s.tcp_flags as u8,
            },
            u64::from(s.length),
        )),
        _ => None,
    }
}

fn header_tuple(h: &SampledHeader) -> Option<Tuple> {
    let b = h.header.as_slice();
    match h.protocol {
        HeaderProtocol::EthernetIso88023 => ethernet(b),
        HeaderProtocol::Ipv4 => ipv4(b),
        HeaderProtocol::Ipv6 => ipv6(b),
        _ => None,
    }
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn ethernet(b: &[u8]) -> Option<Tuple> {
    let mut et = be16(b, 12)?;
    let mut off = 14;
    while et == 0x8100 || et == 0x88a8 {
        et = be16(b, off + 2)?;
        off += 4;
    }
    match et {
        0x0800 => ipv4(b.get(off..)?),
        0x86dd => ipv6(b.get(off..)?),
        _ => None,
    }
}

fn ipv4(b: &[u8]) -> Option<Tuple> {
    if b.len() < 20 || b[0] >> 4 != 4 {
        return None;
    }
    let ihl = usize::from(b[0] & 0x0f) * 4;
    let src = Ipv4Addr::new(b[12], b[13], b[14], b[15]);
    let dst = Ipv4Addr::new(b[16], b[17], b[18], b[19]);
    Some(l4(
        IpAddr::V4(src),
        IpAddr::V4(dst),
        b[9],
        b.get(ihl..).unwrap_or_default(),
    ))
}

fn ipv6(b: &[u8]) -> Option<Tuple> {
    if b.len() < 40 || b[0] >> 4 != 6 {
        return None;
    }
    let src: [u8; 16] = b[8..24].try_into().ok()?;
    let dst: [u8; 16] = b[24..40].try_into().ok()?;
    Some(l4(
        IpAddr::V6(Ipv6Addr::from(src)),
        IpAddr::V6(Ipv6Addr::from(dst)),
        b[6],
        &b[40..],
    ))
}

fn l4(src: IpAddr, dst: IpAddr, proto: u8, b: &[u8]) -> Tuple {
    let ports = matches!(proto, 6 | 17 | 132);
    Tuple {
        src,
        dst,
        sport: if ports { be16(b, 0).unwrap_or(0) } else { 0 },
        dport: if ports { be16(b, 2).unwrap_or(0) } else { 0 },
        proto,
        flags: if proto == 6 {
            b.get(13).copied().unwrap_or(0)
        } else {
            0
        },
    }
}
