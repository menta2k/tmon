//! Arbitrary datagrams into the sFlow v5 decoder must never panic.
#![no_main]

use std::net::SocketAddr;

use libfuzzer_sys::fuzz_target;
use tmon_collector::{ExporterRegistry, SflowDecoder};

fuzz_target!(|data: &[u8]| {
    let src: SocketAddr = "192.0.2.1:6343".parse().unwrap();
    let reg = ExporterRegistry::new();
    let _ = SflowDecoder::new().decode(src, data, &reg, 1_700_000_000_000);
});
