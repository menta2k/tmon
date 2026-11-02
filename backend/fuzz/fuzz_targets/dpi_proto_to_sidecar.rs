//! tmon → classifier messages (feature 003, SC-008): decoding arbitrary
//! bytes never panics, and anything that decodes survives a round trip.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tmon_dpi_proto::ToClassifier;

fuzz_target!(|data: &[u8]| {
    if let Ok(msg) = ToClassifier::decode(data) {
        let again = ToClassifier::decode(&msg.encode()).expect("re-encoded message decodes");
        assert_eq!(again, msg);
    }
});
