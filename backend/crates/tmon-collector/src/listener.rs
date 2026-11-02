//! UDP listeners (FR-005, FR-006): allow-list, version dispatch, bounded queue
//! with drop-and-count back-pressure.

use serde::Serialize;
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tmon_core::RawFlow;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::counters::{DropReason, GlobalCounters};
use crate::exporter_registry::{Exporter, ExporterRegistry};
use crate::netflow::{Decoded, NetflowDecoder};
use crate::sflow::SflowDecoder;

const MAX_DATAGRAM: usize = 65_535;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ListenerKind {
    Netflow,
    Sflow,
}

/// Status of a configured listener (reported by `/status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListenerInfo {
    pub proto: ListenerKind,
    pub port: u16,
    pub address: String,
    pub listening: bool,
    pub error: Option<String>,
}

/// Shared context for all listeners.
#[derive(Clone)]
pub struct ListenerCtx {
    pub registry: Arc<ExporterRegistry>,
    pub globals: Arc<GlobalCounters>,
    /// Batches of flows (one batch per datagram).
    pub tx: mpsc::Sender<Vec<RawFlow>>,
    /// Empty = accept all exporters.
    pub allowed: Arc<Vec<IpAddr>>,
    /// Notified for every newly seen exporter (for persistence).
    pub new_exporters: Option<mpsc::UnboundedSender<Arc<Exporter>>>,
}

impl ListenerCtx {
    fn is_allowed(&self, ip: &IpAddr) -> bool {
        self.allowed.is_empty() || self.allowed.contains(ip)
    }
}

/// Bind a UDP socket with an enlarged receive buffer.
pub fn bind(addr: SocketAddr, recv_buffer: usize) -> io::Result<UdpSocket> {
    let sock = Socket::new(Domain::for_address(addr), Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    if let Err(e) = sock.set_recv_buffer_size(recv_buffer) {
        tracing::warn!(%addr, error = %e, "could not set SO_RCVBUF");
    }
    if addr.is_ipv6() {
        sock.set_only_v6(false).ok();
    }
    sock.set_nonblocking(true)?;
    sock.bind(&addr.into())?;
    UdpSocket::from_std(sock.into())
}

/// Spawn the receive loop for one socket; stops when `shutdown` flips to true.
pub fn spawn(
    sock: UdpSocket,
    kind: ListenerKind,
    ctx: ListenerCtx,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        let mut nf = NetflowDecoder::new();
        let sf = SflowDecoder::new();
        loop {
            let (len, src) = tokio::select! {
                r = sock.recv_from(&mut buf) => match r {
                    Ok(x) => x,
                    Err(e) => {
                        tracing::warn!(error = %e, "udp receive failed");
                        continue;
                    }
                },
                _ = shutdown.changed() => break,
            };
            if !ctx.is_allowed(&src.ip()) {
                ctx.globals.drop_n(DropReason::NotAllowed, 1);
                continue;
            }
            let data = &buf[..len];
            let decoded = match kind {
                ListenerKind::Netflow => nf.decode(src, data, &ctx.registry, now_ms()),
                ListenerKind::Sflow => sf.decode(src, data, &ctx.registry, now_ms()),
            };
            if !dispatch(&ctx, decoded) {
                break;
            }
        }
    })
}

/// Forward decoded flows; returns false when the consumer is gone.
fn dispatch(ctx: &ListenerCtx, d: Decoded) -> bool {
    if d.unknown_version {
        ctx.globals.drop_n(DropReason::UnknownVersion, 1);
    }
    if d.exporter_limit {
        ctx.globals.drop_n(DropReason::ExporterLimit, 1);
    }
    if let Some(tx) = &ctx.new_exporters {
        d.new_exporters.into_iter().for_each(|e| {
            let _ = tx.send(e);
        });
    }
    if d.flows.is_empty() {
        return true;
    }
    match ctx.tx.try_send(d.flows) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(v)) => {
            ctx.globals.drop_n(DropReason::Backpressure, v.len() as u64);
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}
