//! Inputs found by `cargo fuzz` (backend/fuzz) that once panicked.
use std::net::SocketAddr;

use tmon_collector::fields::MAX_FLOW_COUNT;
use tmon_collector::{ExporterRegistry, NetflowDecoder};

#[test]
fn huge_counters_do_not_overflow_and_are_clamped() {
    let data = include_bytes!("regress/netflow-sum-overflow.bin");
    let src: SocketAddr = "192.0.2.1:2055".parse().unwrap();
    let reg = ExporterRegistry::new();
    let out = NetflowDecoder::new().decode(src, data, &reg, 1_700_000_000_000);
    assert!(
        out.flows
            .iter()
            .all(|f| f.bytes <= MAX_FLOW_COUNT && f.packets <= MAX_FLOW_COUNT)
    );
}

#[test]
fn extreme_uptimes_do_not_overflow_time_math() {
    let data = include_bytes!("regress/netflow-time-overflow.bin");
    let src: SocketAddr = "192.0.2.1:2055".parse().unwrap();
    let reg = ExporterRegistry::new();
    let _ = NetflowDecoder::new().decode(src, data, &reg, 1_700_000_000_000);
}
