//! Arbitrary bytes → a sequence of protocol messages → the sidecar flow
//! table with the real nDPI (feature 003, SC-008). Under ASan this covers
//! nDPI's dissectors on adversarial packets within the sidecar's bounds.
//!
//! Input: `[len_hi, len_lo, message…]*` (one encoded `ToClassifier` each).
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::cell::RefCell;
use tmon_dpi::flows::{Flows, Limits};
use tmon_dpi::ndpi::Module;
use tmon_dpi_proto::ToClassifier;

const LIMITS: Limits = Limits {
    max_flows: 1_024,
    max_packets: 32,
    idle_timeout_ms: 60_000,
    mem_limit_bytes: 64 << 20,
};

thread_local! {
    static FLOWS: RefCell<Flows<Module>> = RefCell::new({
        tmon_dpi::alloc::install();
        Flows::new(Module::new().expect("nDPI init"), LIMITS)
    });
}

fuzz_target!(|data: &[u8]| {
    FLOWS.with_borrow_mut(|flows| {
        let mut rest = data;
        while rest.len() >= 2 {
            let len = (usize::from(rest[0]) << 8 | usize::from(rest[1])).min(rest.len() - 2);
            match ToClassifier::decode(&rest[2..2 + len]) {
                Ok(ToClassifier::Packet(p)) => drop(flows.packet(&p)),
                Ok(ToClassifier::Forget(k)) => flows.forget(&k),
                Ok(ToClassifier::Ping(_)) => {}
                Err(_) => flows.invalid(),
            }
            rest = &rest[2 + len..];
        }
        assert!(flows.active() <= LIMITS.max_flows);
    });
});
