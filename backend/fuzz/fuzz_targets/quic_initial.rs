//! QUIC long headers, Initial decryption and CRYPTO frame walking.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tmon_capture::quic::{self, InitialKeys, crypto_frames};

fuzz_target!(|data: &[u8]| {
    for p in quic::packets(data).into_iter().flatten() {
        if let Ok(Some(v)) = p.initial_version() {
            if let Ok(plain) = quic::decrypt(&p, &InitialKeys::client(v, p.header.dcid)) {
                let _ = crypto_frames::crypto_chunks(&plain);
            }
        }
    }
    let _ = crypto_frames::crypto_chunks(data);
});
