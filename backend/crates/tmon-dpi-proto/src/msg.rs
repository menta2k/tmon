//! Message types shared by both directions.

use std::net::IpAddr;

use crate::limits::{MAX_HOST, MAX_NAME};
use crate::wire::{DecodeError, Reader, Writer};

/// LAN-side connection identity (same as tmon's `ConnKey`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub lan_ip: IpAddr,
    pub lan_port: u16,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
    pub proto: u8,
}

impl Key {
    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let lan_ip = r.ip()?;
        let lan_port = r.u16()?;
        let remote_ip = r.ip()?;
        let remote_port = r.u16()?;
        let proto = r.u8()?;
        if lan_ip.is_ipv4() != remote_ip.is_ipv4() {
            return Err(DecodeError::Invalid("key family"));
        }
        Ok(Self {
            lan_ip,
            lan_port,
            remote_ip,
            remote_port,
            proto,
        })
    }

    pub(crate) fn write(&self, w: &mut Writer) {
        w.ip(&self.lan_ip);
        w.u16(self.lan_port);
        w.ip(&self.remote_ip);
        w.u16(self.remote_port);
        w.u8(self.proto);
    }
}

/// Packet direction relative to the LAN side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    LanToRemote = 0,
    RemoteToLan = 1,
}

impl Dir {
    pub(crate) fn from_u8(v: u8) -> Result<Self, DecodeError> {
        match v {
            0 => Ok(Dir::LanToRemote),
            1 => Ok(Dir::RemoteToLan),
            _ => Err(DecodeError::Invalid("dir")),
        }
    }
}

/// One forwarded L3 packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub key: Key,
    pub dir: Dir,
    pub ts_ms: u64,
    /// IPv4 or IPv6 packet starting at the IP header.
    pub l3: Vec<u8>,
}

/// tmon → classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToClassifier {
    Packet(Packet),
    Forget(Key),
    Ping(u64),
}

/// One application known to the classifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CatalogEntry {
    pub app: String,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub proto_version: u16,
    pub impl_name: String,
    pub impl_version: String,
    pub engine: String,
    pub max_flows: u32,
    pub catalog: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub key: Key,
    pub app: String,
    pub category: String,
    pub confident: bool,
    pub final_: bool,
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopApp {
    pub app: String,
    pub flows: u32,
}

/// Classifier counters; also the heartbeat.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub uptime_ms: u64,
    pub packets_in: u64,
    pub packets_invalid: u64,
    pub flows_active: u32,
    pub flows_created: u64,
    pub evicted_lru: u64,
    pub evicted_idle: u64,
    pub evicted_memory: u64,
    pub classified_confident: u64,
    pub classified_guess: u64,
    pub giveups: u64,
    pub mem_bytes: u64,
    pub mem_limit_bytes: u64,
    pub top: Vec<TopApp>,
}

/// Classifier → tmon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromClassifier {
    Hello(Hello),
    Classification(Classification),
    Stats(Stats),
    HelloCatalog(Vec<CatalogEntry>),
    Pong(u64),
}

pub(crate) fn read_entry(r: &mut Reader<'_>) -> Result<CatalogEntry, DecodeError> {
    Ok(CatalogEntry {
        app: non_empty(r.str8(MAX_NAME, "catalog app")?, "catalog app")?,
        category: r.str8(MAX_NAME, "catalog category")?,
    })
}

pub(crate) fn write_entry(w: &mut Writer, e: &CatalogEntry) {
    w.str8(&e.app, MAX_NAME);
    w.str8(&e.category, MAX_NAME);
}

pub(crate) fn non_empty(s: String, what: &'static str) -> Result<String, DecodeError> {
    if s.is_empty() {
        Err(DecodeError::Invalid(what))
    } else {
        Ok(s)
    }
}

pub(crate) fn read_host(r: &mut Reader<'_>) -> Result<Option<String>, DecodeError> {
    let h = r.str8(MAX_HOST, "host")?;
    Ok((!h.is_empty()).then_some(h))
}
