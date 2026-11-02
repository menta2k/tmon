#![allow(dead_code)]
use std::net::SocketAddr;
use std::sync::Arc;
use tmon_capture::Limits;
use tmon_capture::health::{CounterValues, Health};
use tmon_capture::pending::PendingRemoteNames;
use tmon_capture::processor::{CaptureProcessor, Outputs};
use tmon_capture::table::ConnTable;
use tmon_core::naming::{ConnKey, ConnName, ConnectionNamer};

pub const NOW: i64 = 1_767_225_600_000;

pub struct Rig {
    pub out: Outputs,
    pub proc: CaptureProcessor,
}

impl Rig {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        let health = Arc::new(Health::default());
        let out = Outputs {
            table: Arc::new(ConnTable::new(&limits, health.clone())),
            health,
            names: Arc::new(PendingRemoteNames::default()),
        };
        let proc = CaptureProcessor::new(out.clone(), limits);
        Self { out, proc }
    }

    /// Same rig with a classifier tap installed (feature 003).
    pub fn with_tap(self, tap: tmon_capture::TapConfig) -> Self {
        Self {
            proc: self.proc.with_tap(tap),
            out: self.out,
        }
    }

    pub fn feed(&mut self, datagrams: &[Vec<u8>]) {
        for d in datagrams {
            self.proc.handle(d, NOW);
        }
    }

    pub fn counters(&self) -> CounterValues {
        self.out.health.counters.values()
    }

    pub fn name(&self, client: SocketAddr, server: SocketAddr, proto: u8) -> Option<Arc<ConnName>> {
        let key = ConnKey {
            lan_ip: client.ip(),
            lan_port: client.port(),
            remote_ip: server.ip(),
            remote_port: server.port(),
            proto,
        };
        self.out.table.lookup(&key, NOW).connection
    }
}
