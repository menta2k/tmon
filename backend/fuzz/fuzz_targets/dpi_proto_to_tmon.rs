//! classifier → tmon messages (feature 003, SC-008): tmon's side of the
//! trust boundary. Decoding arbitrary bytes never panics, and anything that
//! decodes survives a round trip.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tmon_dpi_proto::FromClassifier;

fuzz_target!(|data: &[u8]| {
    if let Ok(msg) = FromClassifier::decode(data) {
        let again = FromClassifier::decode(&msg.encode()).expect("re-encoded message decodes");
        assert_eq!(again, msg);
    }
});
