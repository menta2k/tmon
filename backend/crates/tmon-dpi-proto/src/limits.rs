//! Decode limits (contracts/dpi-ipc.md, "Limits").

/// Largest message on the socket, in bytes.
pub const MAX_MESSAGE: usize = 9_400;
/// Smallest and largest forwarded L3 packet.
pub const MIN_L3: usize = 20;
pub const MAX_L3: usize = 9_280;
/// Default `str8` limit.
pub const MAX_STR: usize = 64;
/// Application and category names.
pub const MAX_NAME: usize = 48;
/// Host names.
pub const MAX_HOST: usize = 253;
/// Catalog entries across Hello + HelloCatalog messages.
pub const MAX_CATALOG: usize = 2_048;
/// Entries in `Stats.top`.
pub const MAX_TOP: usize = 16;
/// Classifications per second tmon accepts.
pub const MAX_CLASSIFICATIONS_PER_SEC: u32 = 50_000;

/// Message type codes.
pub mod kind {
    pub const PACKET: u8 = 0x01;
    pub const FORGET: u8 = 0x02;
    pub const PING: u8 = 0x03;
    pub const HELLO: u8 = 0x81;
    pub const CLASSIFICATION: u8 = 0x82;
    pub const STATS: u8 = 0x83;
    pub const HELLO_CATALOG: u8 = 0x84;
    pub const PONG: u8 = 0x85;
}
