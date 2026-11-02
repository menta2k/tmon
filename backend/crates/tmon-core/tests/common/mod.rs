// Shared test helpers (included via `mod common;`).
#![allow(dead_code)]

use tmon_core::{Direction, FlowRecord, RawFlow};

pub fn raw(src: &str, dst: &str, sport: u16, dport: u16, proto: u8) -> RawFlow {
    RawFlow {
        ts_ms: 1_700_000_000_000,
        ts_start_ms: 1_700_000_000_000,
        exporter_id: 1,
        in_if: 1,
        out_if: 2,
        src_ip: src.parse().unwrap(),
        dst_ip: dst.parse().unwrap(),
        src_port: sport,
        dst_port: dport,
        proto,
        tcp_flags: 0,
        bytes: 1000,
        packets: 10,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    }
}

/// An outbound ("up") flow from a local host to a remote endpoint.
pub fn flow(local: &str, remote: &str, rport: u16, proto: u8) -> FlowRecord {
    FlowRecord::from_raw(
        &raw(local, remote, 50000, rport, proto),
        Direction::Up,
        true,
    )
}
