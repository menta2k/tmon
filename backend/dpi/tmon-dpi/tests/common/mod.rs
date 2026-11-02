#![allow(dead_code)]
//! A scripted engine: the last L3 byte selects the result. Live flows are
//! counted (1 KB each) so freeing and the memory budget are observable.

use std::cell::Cell;
use std::net::IpAddr;
use std::rc::Rc;
use tmon_dpi::engine::{Engine, NdpiConfidence, NdpiResult};
use tmon_dpi::flows::Limits;
use tmon_dpi_proto::{Dir, Key, Packet};

pub const FLOW_BYTES: u64 = 1_000;
pub const UNKNOWN: u8 = 0;
pub const GUESS: u8 = 1;
pub const CONFIDENT: u8 = 2;
pub const CARRIER: u8 = 3;

pub struct MockFlow {
    live: Rc<Cell<u64>>,
}

impl Drop for MockFlow {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}

#[derive(Default)]
pub struct Mock {
    pub live: Rc<Cell<u64>>,
    pub giveups: Cell<u32>,
}

fn result(app: &str, master: &str, c: Option<NdpiConfidence>) -> NdpiResult {
    NdpiResult {
        app: app.into(),
        master: master.into(),
        category: "VPN".into(),
        confidence: c,
        host: String::new(),
    }
}

impl Engine for Mock {
    type Flow = MockFlow;

    fn new_flow(&mut self) -> Option<MockFlow> {
        self.live.set(self.live.get() + 1);
        Some(MockFlow {
            live: Rc::clone(&self.live),
        })
    }

    fn process(&mut self, _f: &mut MockFlow, l3: &[u8], _ts: u64) -> NdpiResult {
        match l3.last().copied().unwrap_or(UNKNOWN) {
            GUESS => result("Unknown", "WireGuard", Some(NdpiConfidence::MatchByPort)),
            CONFIDENT => result("BitTorrent", "Unknown", Some(NdpiConfidence::Dpi)),
            CARRIER => result("TLS", "Unknown", Some(NdpiConfidence::Dpi)),
            _ => result("Unknown", "Unknown", None),
        }
    }

    fn giveup(&mut self, _f: &mut MockFlow) -> NdpiResult {
        self.giveups.set(self.giveups.get() + 1);
        result("WireGuard", "Unknown", Some(NdpiConfidence::MatchByPort))
    }

    fn mem_bytes(&self) -> u64 {
        self.live.get() * FLOW_BYTES
    }
}

pub fn limits() -> Limits {
    Limits {
        max_flows: 4,
        max_packets: 3,
        idle_timeout_ms: 60_000,
        mem_limit_bytes: 0,
    }
}

pub fn key(port: u16) -> Key {
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    Key {
        lan_ip: ip("192.168.88.10"),
        lan_port: port,
        remote_ip: ip("198.51.100.20"),
        remote_port: 51820,
        proto: 17,
    }
}

/// An IPv4/UDP packet whose last payload byte is the script `code`.
pub fn pkt(port: u16, code: u8, ts_ms: u64) -> Packet {
    let mut l3 = vec![
        0x45, 0, 0, 29, 0, 0, 0x40, 0, 64, 17, 0, 0, 192, 168, 88, 10, 198, 51, 100, 20,
    ];
    l3.extend_from_slice(&port.to_be_bytes());
    l3.extend_from_slice(&51820u16.to_be_bytes());
    l3.extend_from_slice(&[0, 9, 0, 0, code]);
    Packet {
        key: key(port),
        dir: Dir::LanToRemote,
        ts_ms,
        l3,
    }
}
