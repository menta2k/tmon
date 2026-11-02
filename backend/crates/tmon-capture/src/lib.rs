//! tmon-capture: names connections from router packet capture (TZSP).
//!
//! Hostile-input surface: every parser is bounds-checked, every rejection is
//! counted, and nothing here keeps payload bytes beyond bounded reassembly.

pub mod dns;
pub mod frame;
pub mod health;
pub mod http;
pub mod listener;
pub mod pending;
pub mod processor;
pub mod quic;
pub mod settings;
pub mod table;
pub mod tap;
pub mod tcp_reasm;
pub mod tls;
pub mod tzsp;

#[cfg(feature = "fixtures")]
pub mod fixtures;

pub use listener::{CaptureHandle, CaptureStats};
pub use settings::{CaptureSettings, Limits};
pub use tap::{PacketTap, TapConfig, TapDir};
