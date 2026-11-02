//! Arbitrary datagrams into the NetFlow v5/v9/IPFIX decoder must never panic.
//! Template state is kept across inputs (as in production) so v9/IPFIX data
//! sets are decoded against templates learned from earlier inputs.
#![no_main]

use std::cell::RefCell;
use std::net::SocketAddr;

use libfuzzer_sys::fuzz_target;
use tmon_collector::{ExporterRegistry, NetflowDecoder};

thread_local! {
    static STATE: RefCell<(NetflowDecoder, ExporterRegistry)> =
        RefCell::new((NetflowDecoder::new(), ExporterRegistry::new()));
}

fuzz_target!(|data: &[u8]| {
    let src: SocketAddr = "192.0.2.1:2055".parse().unwrap();
    STATE.with(|s| {
        let (dec, reg) = &mut *s.borrow_mut();
        let out = dec.decode(src, data, reg, 1_700_000_000_000);
        assert!(out.flows.len() <= 65_535);
    });
});
