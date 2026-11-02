//! Arbitrary datagram sequences through one stateful capture processor
//! (TZSP → frame → TCP reassembly / QUIC / DNS / HTTP) must never panic,
//! hang or exceed the reassembly bounds.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::sync::Arc;
use tmon_capture::Limits;
use tmon_capture::health::Health;
use tmon_capture::pending::PendingRemoteNames;
use tmon_capture::processor::{CaptureProcessor, Outputs};
use tmon_capture::table::ConnTable;

fuzz_target!(|data: &[u8]| {
    let limits = Limits {
        max_connections: 64,
        max_reassembly_bytes_per_conn: 4096,
        max_reassembly_bytes_total: 16_384,
        max_device_bindings: 64,
        ..Limits::default()
    };
    let health = Arc::new(Health::default());
    let out = Outputs {
        table: Arc::new(ConnTable::new(&limits, health.clone())),
        health,
        names: Arc::new(PendingRemoteNames::default()),
    };
    let mut p = CaptureProcessor::new(out, limits);
    // Input = sequence of [len_hi, len_lo, bytes…] datagrams.
    let mut rest = data;
    let mut now = 1_700_000_000_000i64;
    while rest.len() >= 2 {
        let len = (usize::from(rest[0]) << 8 | usize::from(rest[1])).min(rest.len() - 2);
        p.handle(&rest[2..2 + len], now);
        assert!(
            p.reassembly_bytes()
                <= limits.max_reassembly_bytes_total + limits.max_reassembly_bytes_per_conn
        );
        rest = &rest[2 + len..];
        now += 700;
    }
});
