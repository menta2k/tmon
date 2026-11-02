//! tmon ↔ application classifier IPC, protocol version 1.
//!
//! The wire format is specified in
//! `specs/003-ndpi-app-classification/contracts/dpi-ipc.md` and is public
//! (FR-027). Decoding treats every input as hostile: all lengths are bounded
//! before allocation, and malformed input yields an error, never a panic.

mod limits;
mod msg;
mod to_classifier;
mod to_tmon;
mod wire;

pub use limits::*;
pub use msg::{
    CatalogEntry, Classification, Dir, FromClassifier, Hello, Key, Packet, Stats, ToClassifier,
    TopApp,
};
pub use wire::DecodeError;

/// Protocol version spoken by this crate.
pub const PROTO_VERSION: u16 = 1;
