//! Field-number based extraction shared by NetFlow v9 and IPFIX records.
//! (IANA IPFIX element ids coincide with v9 field types for these fields.)

use netflow_parser::FieldValue;
use netflow_parser::variable_versions::field_value::DurationValue;
use std::net::IpAddr;

/// Seconds between the NTP epoch (1900) and the Unix epoch (1970).
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

/// Upper bound for a single flow's (sampling-scaled) bytes/packets (≈1 TB).
/// Hostile or corrupt exports must not overflow `sum(bytes)` in storage rollups.
pub const MAX_FLOW_COUNT: u64 = 1 << 40;

/// Sampling-scale a counter, clamped to [`MAX_FLOW_COUNT`].
pub fn scaled(v: u64, rate: u64) -> u64 {
    v.saturating_mul(rate).min(MAX_FLOW_COUNT)
}

/// Values of interest pulled from one flow record.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Fields {
    pub src: Option<IpAddr>,
    pub dst: Option<IpAddr>,
    pub sport: Option<u16>,
    pub dport: Option<u16>,
    pub proto: Option<u8>,
    pub tcp_flags: u8,
    pub bytes: Option<u64>,
    pub packets: Option<u64>,
    pub in_if: u32,
    pub out_if: u32,
    pub first_uptime: Option<u64>,
    pub last_uptime: Option<u64>,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub sys_init_ms: Option<u64>,
    pub sampling: Option<u32>,
    pub nat_ip: Option<IpAddr>,
    pub nat_port: Option<u16>,
    pub nat_dst_ip: Option<IpAddr>,
    pub nat_dst_port: Option<u16>,
    pub flow_direction: Option<u8>,
}

fn num(v: &FieldValue) -> Option<u64> {
    match v {
        FieldValue::DataNumber(_) => v.as_u64(),
        FieldValue::Duration(d) => duration_ms(d),
        FieldValue::ProtocolType(p) => Some(u64::from(u8::from(*p))),
        _ => None,
    }
}

fn duration_ms(d: &DurationValue) -> Option<u64> {
    match d {
        DurationValue::Millis { value, .. } => Some(*value),
        DurationValue::Seconds { value, .. } => value.checked_mul(1000),
        DurationValue::MicrosNtp { seconds, .. } | DurationValue::NanosNtp { seconds, .. } => {
            u64::from(*seconds)
                .checked_sub(NTP_UNIX_OFFSET)
                .map(|s| s * 1000)
        }
    }
}

fn ip(v: &FieldValue) -> Option<IpAddr> {
    match v {
        FieldValue::Ip4Addr(a) => Some(IpAddr::V4(*a)),
        FieldValue::Ip6Addr(a) => Some(IpAddr::V6(*a)),
        _ => None,
    }
}

fn flags(v: &FieldValue) -> u8 {
    match v {
        FieldValue::TcpControlBits(t, _) => {
            let bits = [t.fin, t.syn, t.rst, t.psh, t.ack, t.urg, t.ece, t.cwr];
            bits.iter()
                .enumerate()
                .fold(0u8, |acc, (i, b)| if *b { acc | (1 << i) } else { acc })
        }
        other => num(other).map_or(0, |n| (n & 0xff) as u8),
    }
}

fn u16_of(v: &FieldValue) -> Option<u16> {
    num(v).and_then(|n| u16::try_from(n).ok())
}

fn u32_of(v: &FieldValue) -> u32 {
    num(v).and_then(|n| u32::try_from(n).ok()).unwrap_or(0)
}

impl Fields {
    /// Fold `(field id, value)` pairs into a `Fields` value.
    pub fn from_pairs<'a>(pairs: impl Iterator<Item = (u16, &'a FieldValue)>) -> Self {
        pairs.fold(Fields::default(), |f, (id, v)| f.with(id, v))
    }

    fn with(self, id: u16, v: &FieldValue) -> Self {
        match id {
            1 | 85 => Self {
                bytes: self.bytes.or(num(v)),
                ..self
            },
            2 | 86 => Self {
                packets: self.packets.or(num(v)),
                ..self
            },
            4 => Self {
                proto: num(v).and_then(|n| u8::try_from(n).ok()),
                ..self
            },
            6 => Self {
                tcp_flags: flags(v),
                ..self
            },
            7 => Self {
                sport: u16_of(v),
                ..self
            },
            8 | 27 => Self {
                src: self.src.or(ip(v)),
                ..self
            },
            10 => Self {
                in_if: u32_of(v),
                ..self
            },
            11 => Self {
                dport: u16_of(v),
                ..self
            },
            12 | 28 => Self {
                dst: self.dst.or(ip(v)),
                ..self
            },
            14 => Self {
                out_if: u32_of(v),
                ..self
            },
            21 => Self {
                last_uptime: num(v),
                ..self
            },
            22 => Self {
                first_uptime: num(v),
                ..self
            },
            34 | 305 => Self {
                sampling: num(v).and_then(|n| u32::try_from(n).ok()),
                ..self
            },
            150 | 152 => Self {
                start_ms: self.start_ms.or(num(v)),
                ..self
            },
            151 | 153 => Self {
                end_ms: self.end_ms.or(num(v)),
                ..self
            },
            160 => Self {
                sys_init_ms: num(v),
                ..self
            },
            225 | 281 => Self {
                nat_ip: ip(v).filter(|a| !a.is_unspecified()),
                ..self
            },
            61 => Self {
                flow_direction: num(v).and_then(|n| u8::try_from(n).ok()),
                ..self
            },
            226 | 282 => Self {
                nat_dst_ip: ip(v).filter(|a| !a.is_unspecified()),
                ..self
            },
            227 => Self {
                nat_port: u16_of(v).filter(|p| *p != 0),
                ..self
            },
            228 => Self {
                nat_dst_port: u16_of(v).filter(|p| *p != 0),
                ..self
            },
            _ => self,
        }
    }

    /// Absolute (start, end) in epoch ms.
    /// `export_ms` = header export time, `uptime_ms` = header sysUpTime if known.
    pub fn times(&self, export_ms: i64, uptime_ms: Option<u64>) -> (i64, i64) {
        let as_i64 = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
        if let (Some(s), Some(e)) = (self.start_ms, self.end_ms) {
            return (as_i64(s), as_i64(e));
        }
        let (first, last) = match (self.first_uptime, self.last_uptime) {
            (Some(f), Some(l)) => (as_i64(f), as_i64(l)),
            _ => return (export_ms, export_ms),
        };
        if let Some(init) = self.sys_init_ms {
            let init = as_i64(init);
            return (init.saturating_add(first), init.saturating_add(last));
        }
        match uptime_ms {
            Some(up) => {
                let up = as_i64(up);
                (
                    export_ms.saturating_sub(up.saturating_sub(first)),
                    export_ms.saturating_sub(up.saturating_sub(last)),
                )
            }
            None => (
                export_ms.saturating_sub(last.saturating_sub(first).max(0)),
                export_ms,
            ),
        }
    }
}
