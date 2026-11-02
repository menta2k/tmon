//! Raw, generated bindings to libndpi (see `build.rs`). Everything here is
//! `unsafe`; the safe wrapper is `tmon-dpi/src/ndpi.rs`.
#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code
)]
#![allow(clippy::all, clippy::undocumented_unsafe_blocks)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
