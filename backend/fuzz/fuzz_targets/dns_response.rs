//! DNS responses → bindings.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tmon_capture::dns;

fuzz_target!(|data: &[u8]| {
    if let Ok(bindings) = dns::bindings(data) {
        for b in bindings {
            assert!(b.aliases.len() <= tmon_core::naming::MAX_ALIASES);
            assert!((dns::MIN_TTL_SECS..=dns::MAX_TTL_SECS).contains(&b.ttl_secs));
        }
    }
});
