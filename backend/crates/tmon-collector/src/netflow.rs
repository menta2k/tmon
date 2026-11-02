//! NetFlow v5 / v9 / IPFIX decoding into `RawFlow` (FR-001, FR-003, FR-004).

use netflow_parser::static_versions::v5::V5;
use netflow_parser::variable_versions::ipfix::lookup::{IANAIPFixField, IPFixField};
use netflow_parser::variable_versions::ipfix::{FlowSetBody as IpfixBody, IPFix};
use netflow_parser::variable_versions::v9::{FlowSetBody as V9Body, V9};
use netflow_parser::{AutoScopedParser, NetflowPacket};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::LazyLock;
use tmon_core::RawFlow;

use crate::exporter_registry::{Exporter, ExporterRegistry, FlowProtocol};
use crate::fields::Fields;

/// Exporter timestamps further than this from "now" are replaced by receive time.
pub const MAX_SKEW_MS: i64 = 5 * 60 * 1000;

/// Result of decoding one datagram.
#[derive(Debug, Default)]
pub struct Decoded {
    pub flows: Vec<RawFlow>,
    pub new_exporters: Vec<Arc<Exporter>>,
    pub unknown_version: bool,
    /// The datagram came from a new exporter beyond `MAX_EXPORTERS`.
    pub exporter_limit: bool,
}

/// Peek protocol and observation domain from the datagram header.
pub fn peek_header(data: &[u8]) -> Option<(FlowProtocol, u32)> {
    let be32 = |at: usize| {
        data.get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    match u16::from_be_bytes([*data.first()?, *data.get(1)?]) {
        5 => Some((FlowProtocol::Netflow5, 0)),
        9 => Some((FlowProtocol::Netflow9, be32(16).unwrap_or(0))),
        10 => Some((FlowProtocol::Ipfix, be32(12).unwrap_or(0))),
        _ => None,
    }
}

/// Stateful decoder: template caches are scoped per source address + domain.
pub struct NetflowDecoder {
    parser: AutoScopedParser,
}

impl Default for NetflowDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl NetflowDecoder {
    pub fn new() -> Self {
        Self {
            parser: AutoScopedParser::new(),
        }
    }

    /// Decode a datagram from `src`; counters on the exporter are updated.
    pub fn decode(
        &mut self,
        src: SocketAddr,
        data: &[u8],
        reg: &ExporterRegistry,
        now_ms: i64,
    ) -> Decoded {
        let Some((protocol, domain)) = peek_header(data) else {
            return Decoded {
                unknown_version: true,
                ..Decoded::default()
            };
        };
        let Some((exporter, is_new)) = reg.get_or_create(src.ip(), domain, protocol, now_ms) else {
            return Decoded {
                exporter_limit: true,
                ..Decoded::default()
            };
        };
        exporter.counters.add_received();
        exporter.touch(protocol, now_ms);
        let result = self.parser.parse_from_source(src, data);
        if result.error.is_some() {
            exporter.counters.add_parse_error();
        }
        let mut flows = Vec::new();
        for packet in &result.packets {
            match packet {
                NetflowPacket::V5(v5) => decode_v5(v5, &exporter, now_ms, &mut flows),
                NetflowPacket::V9(v9) => decode_v9(v9, &exporter, now_ms, &mut flows),
                NetflowPacket::IPFix(ipfix) => decode_ipfix(ipfix, &exporter, now_ms, &mut flows),
                _ => {}
            }
        }
        let bytes = flows
            .iter()
            .fold(0u64, |acc, f| acc.saturating_add(f.bytes));
        exporter.counters.add_decoded(flows.len() as u64, bytes);
        let new_exporters = if is_new { vec![exporter] } else { Vec::new() };
        Decoded {
            flows,
            new_exporters,
            ..Decoded::default()
        }
    }
}

/// Clamp implausible exporter timestamps to receive time (edge case: clock skew).
pub fn fix_skew(start: i64, end: i64, now_ms: i64) -> (i64, i64) {
    if end.saturating_sub(now_ms).saturating_abs() > MAX_SKEW_MS {
        let dur = end.saturating_sub(start).clamp(0, MAX_SKEW_MS);
        (now_ms - dur, now_ms)
    } else {
        (start.min(end), end)
    }
}

fn build(
    f: &Fields,
    exporter: &Exporter,
    export_ms: i64,
    uptime: Option<u64>,
    now_ms: i64,
) -> Option<RawFlow> {
    let (src, dst) = (f.src?, f.dst?);
    let rate = u64::from(
        f.sampling
            .filter(|_| exporter.settings().sampling_override.is_none())
            .unwrap_or_else(|| exporter.sampling_rate())
            .max(1),
    );
    let (start, end) = f.times(export_ms, uptime);
    let (ts_start_ms, ts_ms) = fix_skew(start, end, now_ms);
    Some(RawFlow {
        ts_ms,
        ts_start_ms,
        exporter_id: exporter.id,
        in_if: f.in_if,
        out_if: f.out_if,
        src_ip: src,
        dst_ip: dst,
        src_port: f.sport.unwrap_or(0),
        dst_port: f.dport.unwrap_or(0),
        proto: f.proto.unwrap_or(0),
        tcp_flags: f.tcp_flags,
        bytes: crate::fields::scaled(f.bytes.unwrap_or(0), rate),
        packets: crate::fields::scaled(f.packets.unwrap_or(0), rate),
        sampled: rate > 1,
        // Exporters repeat the original endpoint when nothing was translated.
        nat_ip: f.nat_ip.filter(|a| *a != src),
        nat_port: f.nat_ip.filter(|a| *a != src).and(f.nat_port),
        nat_dst_ip: f.nat_dst_ip.filter(|a| *a != dst),
        nat_dst_port: f.nat_dst_ip.filter(|a| *a != dst).and(f.nat_dst_port),
        flow_direction: f.flow_direction,
    })
}

fn decode_v5(v5: &V5, exporter: &Exporter, now_ms: i64, out: &mut Vec<RawFlow>) {
    let h = &v5.header;
    let rate = u32::from(h.sampling_interval & 0x3fff);
    if rate > 1 {
        exporter.set_advertised_sampling(rate);
    }
    let export_ms = i64::from(h.unix_secs) * 1000 + i64::from(h.unix_nsecs / 1_000_000);
    for r in &v5.flowsets {
        let f = Fields {
            src: Some(r.src_addr.into()),
            dst: Some(r.dst_addr.into()),
            sport: Some(r.src_port),
            dport: Some(r.dst_port),
            proto: Some(r.protocol_number),
            tcp_flags: r.tcp_flags,
            bytes: Some(u64::from(r.d_octets)),
            packets: Some(u64::from(r.d_pkts)),
            in_if: u32::from(r.input),
            out_if: u32::from(r.output),
            first_uptime: Some(u64::from(r.first)),
            last_uptime: Some(u64::from(r.last)),
            ..Fields::default()
        };
        out.extend(build(
            &f,
            exporter,
            export_ms,
            Some(u64::from(h.sys_up_time)),
            now_ms,
        ));
    }
}

fn decode_v9(v9: &V9, exporter: &Exporter, now_ms: i64, out: &mut Vec<RawFlow>) {
    let export_ms = i64::from(v9.header.unix_secs) * 1000;
    let uptime = Some(u64::from(v9.header.sys_up_time));
    for set in &v9.flowsets {
        match &set.body {
            V9Body::Data(data) => {
                for rec in &data.fields {
                    let f = Fields::from_pairs(rec.iter().map(|(k, v)| (u16::from(*k), v)));
                    out.extend(build(&f, exporter, export_ms, uptime, now_ms));
                }
            }
            V9Body::OptionsData(opts) => {
                for rec in &opts.fields {
                    let f = Fields::from_pairs(
                        rec.options_fields.iter().map(|(k, v)| (u16::from(*k), v)),
                    );
                    f.sampling
                        .filter(|r| *r > 1)
                        .into_iter()
                        .for_each(|r| exporter.set_advertised_sampling(r));
                }
            }
            V9Body::NoTemplate(_) => exporter.counters.add_template_missing(1),
            _ => {}
        }
    }
}

/// Reverse map IANA element → id (the crate offers no direct conversion).
static IANA_IDS: LazyLock<HashMap<IANAIPFixField, u16>> = LazyLock::new(|| {
    (0..=IANA_MAX_ID)
        .map(|id| (IANAIPFixField::from(id), id))
        .collect()
});
const IANA_MAX_ID: u16 = 1024;

fn ipfix_id(f: &IPFixField) -> Option<u16> {
    match f {
        IPFixField::IANA(IANAIPFixField::Unknown(n)) => Some(*n),
        IPFixField::IANA(x) => IANA_IDS.get(x).copied(),
        _ => None,
    }
}

fn decode_ipfix(p: &IPFix, exporter: &Exporter, now_ms: i64, out: &mut Vec<RawFlow>) {
    let export_ms = i64::from(p.header.export_time) * 1000;
    for set in &p.flowsets {
        match &set.body {
            IpfixBody::Data(data) => {
                for rec in &data.fields {
                    let f = Fields::from_pairs(
                        rec.iter()
                            .filter_map(|(k, v)| ipfix_id(k).map(|id| (id, v))),
                    );
                    out.extend(build(&f, exporter, export_ms, None, now_ms));
                }
            }
            IpfixBody::OptionsData(opts) => {
                for rec in &opts.fields {
                    let f = Fields::from_pairs(
                        rec.iter()
                            .filter_map(|(k, v)| ipfix_id(k).map(|id| (id, v))),
                    );
                    f.sampling
                        .filter(|r| *r > 1)
                        .into_iter()
                        .for_each(|r| exporter.set_advertised_sampling(r));
                }
            }
            IpfixBody::NoTemplate(_) => exporter.counters.add_template_missing(1),
            _ => {}
        }
    }
}
