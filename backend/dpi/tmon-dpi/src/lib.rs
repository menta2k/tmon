//! tmon-dpi: the nDPI application classifier sidecar (feature 003).
//!
//! Pure-Rust core: [`engine`] (the nDPI seam), [`classify`] (result
//! mapping), [`flows`] (per-connection state) and [`ipc`] (the protocol v1
//! loop on fd 3). With the `ndpi` feature: the safe nDPI wrapper, the
//! counting allocator and the sandbox.

pub mod alloc;
pub mod args;
pub mod classify;
pub mod engine;
pub mod flows;
pub mod ipc;

#[cfg(feature = "ndpi")]
pub mod ndpi;
#[cfg(feature = "ndpi")]
pub mod sandbox;
