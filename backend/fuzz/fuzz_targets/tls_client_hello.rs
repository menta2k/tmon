//! ClientHello (record and bare handshake) and HTTP request parsing.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tmon_capture::{http, tls};

fuzz_target!(|data: &[u8]| {
    let _ = tls::looks_like_client_hello(data);
    let _ = tls::record_len(data);
    let _ = tls::handshake_len(data);
    if let Ok(info) = tls::from_record(data) {
        if let Some(sni) = info.sni {
            let _ = tmon_core::naming::normalize_host(&sni);
        }
    }
    let _ = tls::from_handshake(data);
    let _ = http::host(data);
});
