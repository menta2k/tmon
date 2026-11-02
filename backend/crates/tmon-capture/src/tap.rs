//! Packet tap for an external application classifier (feature 003).
//!
//! The processor hands every decoded IP packet to the tap with its
//! LAN-oriented connection key. Implementations must never block: the
//! processor runs on the capture hot path (FR-018).

use std::net::IpAddr;
use std::sync::Arc;
use tmon_core::LocalNetworks;
use tmon_core::naming::ConnKey;

/// Packet direction relative to the LAN side of the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TapDir {
    LanToRemote,
    RemoteToLan,
}

/// Receives copies of decoded packets. `l3` borrows the datagram; copy it
/// if it must outlive the call.
pub trait PacketTap: Send + Sync {
    fn offer(&self, key: &ConnKey, dir: TapDir, ts_ms: i64, l3: &[u8]);
}

/// Is this address on the LAN side?
pub type IsLocal = Arc<dyn Fn(&IpAddr) -> bool + Send + Sync>;

/// A tap plus the check that decides which side is the LAN (research R4):
/// replies (remote → LAN) get the same key as the LAN → remote packets.
#[derive(Clone)]
pub struct TapConfig {
    pub tap: Arc<dyn PacketTap>,
    pub is_local: IsLocal,
}

impl TapConfig {
    /// A tap with a fixed set of local networks.
    pub fn with_networks(tap: Arc<dyn PacketTap>, networks: LocalNetworks) -> Self {
        Self {
            tap,
            is_local: Arc::new(move |ip| networks.contains(ip)),
        }
    }
}
