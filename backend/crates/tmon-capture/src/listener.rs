//! Capture socket → bounded channel → one processor task (R12, FR-015).
//!
//! The socket task never blocks: when the processor falls behind, datagrams
//! are dropped and counted. Flow ingest runs on its own sockets and tasks.

use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};

use crate::health::{Bucket, CounterValues, Health, inc};
use crate::pending::PendingRemoteNames;
use crate::processor::{CaptureProcessor, Outputs};
use crate::settings::CaptureSettings;
use crate::table::ConnTable;
use crate::tap::TapConfig;

const MAX_DATAGRAM: usize = 65_535;
/// Largest TZSP datagram worth decoding: a jumbo frame plus TZSP header.
/// Anything bigger is counted as malformed and never copied.
pub const MAX_CAPTURE_DATAGRAM: usize = 9_216 + 64;
/// Bytes allowed in the processor queue, on top of its slot count.
pub const QUEUE_MAX_BYTES: usize = 16 << 20;
const TICK: Duration = Duration::from_secs(1);
const TABLE_SWEEP_EVERY: u32 = 10;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Bind state of the capture socket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListenState {
    pub listening: bool,
    pub error: Option<String>,
    /// Actual bound address (differs from the setting for port 0).
    pub bound: Option<SocketAddr>,
}

/// Immutable snapshot for `/status`, `/metrics` and the health detectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureStats {
    pub address: SocketAddr,
    pub listening: bool,
    pub error: Option<String>,
    pub counters: CounterValues,
    pub window: Bucket,
    pub last_datagram_ms: Option<i64>,
    pub conns_tracked: u64,
    pub bindings_tracked: u64,
    pub reassembly_bytes: u64,
    /// When capture was started (the silence detector counts from here).
    pub started_ms: i64,
}

/// Everything the rest of tmon sees of a running capture input.
pub struct CaptureHandle {
    pub settings: CaptureSettings,
    pub health: Arc<Health>,
    pub table: Arc<ConnTable>,
    pub names: Arc<PendingRemoteNames>,
    state: Mutex<ListenState>,
    reassembly_bytes: AtomicU64,
    /// Bytes currently waiting in the processor queue.
    queued_bytes: AtomicUsize,
    started_ms: i64,
}

impl CaptureHandle {
    pub fn new(settings: CaptureSettings, now_ms: i64) -> Arc<Self> {
        let health = Arc::new(Health::default());
        Arc::new(Self {
            table: Arc::new(ConnTable::new(&settings.limits, health.clone())),
            names: Arc::new(PendingRemoteNames::default()),
            health,
            settings,
            state: Mutex::new(ListenState::default()),
            reassembly_bytes: AtomicU64::new(0),
            queued_bytes: AtomicUsize::new(0),
            started_ms: now_ms,
        })
    }

    pub fn outputs(&self) -> Outputs {
        Outputs {
            table: Arc::clone(&self.table),
            health: Arc::clone(&self.health),
            names: Arc::clone(&self.names),
        }
    }

    fn set_state(&self, s: ListenState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }

    pub fn state(&self) -> ListenState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn stats(&self, now_ms: i64) -> CaptureStats {
        let st = self.state();
        CaptureStats {
            address: st.bound.unwrap_or(self.settings.listen),
            listening: st.listening,
            error: st.error,
            counters: self.health.counters.values(),
            window: self.health.window(now_ms),
            last_datagram_ms: self.health.last_datagram_ms(),
            conns_tracked: self.table.conns_len() as u64,
            bindings_tracked: self.table.bindings_len() as u64,
            reassembly_bytes: self.reassembly_bytes.load(Relaxed),
            started_ms: self.started_ms,
        }
    }

    /// Current health warnings (R10).
    pub fn warnings(&self, now_ms: i64) -> Vec<crate::health::CaptureWarning> {
        crate::health::warnings(&self.stats(now_ms), self.settings.silent_after, now_ms)
    }

    /// Sender allowed? Empty list = everyone (IPv4-mapped IPv6 is compared as IPv4).
    pub fn accepts(&self, src: IpAddr) -> bool {
        let src = src.to_canonical();
        let allowed = &self.settings.allowed_sources;
        allowed.is_empty() || allowed.iter().any(|a| a.to_canonical() == src)
    }
}

/// Bind a UDP socket with an enlarged receive buffer.
pub fn bind(addr: SocketAddr, recv_buffer: usize) -> io::Result<UdpSocket> {
    let sock = Socket::new(Domain::for_address(addr), Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    if let Err(e) = sock.set_recv_buffer_size(recv_buffer) {
        tracing::warn!(%addr, error = %e, "could not set capture SO_RCVBUF");
    }
    if addr.is_ipv6() {
        sock.set_only_v6(false).ok();
    }
    sock.set_nonblocking(true)?;
    sock.bind(&addr.into())?;
    UdpSocket::from_std(sock.into())
}

/// Start capture: bind, then run the receive and processing tasks until
/// `shutdown` flips. A bind failure is recorded on the handle, not fatal.
pub fn start(settings: CaptureSettings, shutdown: watch::Receiver<bool>) -> Arc<CaptureHandle> {
    start_with_tap(settings, shutdown, None)
}

/// [`start`] with an optional classifier tap (feature 003).
pub fn start_with_tap(
    settings: CaptureSettings,
    shutdown: watch::Receiver<bool>,
    tap: Option<TapConfig>,
) -> Arc<CaptureHandle> {
    let handle = CaptureHandle::new(settings, now_ms());
    let s = &handle.settings;
    match bind(s.listen, s.socket_recv_buffer_bytes) {
        Ok(sock) => {
            if s.allowed_sources.is_empty() {
                tracing::warn!(address = %s.listen, "capture accepting from all senders");
            }
            tracing::info!(address = %s.listen, "capture listening");
            let bound = sock.local_addr().ok();
            handle.set_state(ListenState {
                listening: true,
                error: None,
                bound,
            });
            run_with_tap(sock, Arc::clone(&handle), shutdown, tap);
        }
        Err(e) => {
            tracing::error!(address = %s.listen, error = %e, "capture listener failed to bind");
            handle.set_state(ListenState {
                listening: false,
                error: Some(e.to_string()),
                bound: None,
            });
        }
    }
    handle
}

/// Spawn the receive and processing tasks on an already bound socket.
pub fn run(sock: UdpSocket, handle: Arc<CaptureHandle>, shutdown: watch::Receiver<bool>) {
    run_with_tap(sock, handle, shutdown, None);
}

/// [`run`] with an optional classifier tap.
pub fn run_with_tap(
    sock: UdpSocket,
    handle: Arc<CaptureHandle>,
    shutdown: watch::Receiver<bool>,
    tap: Option<TapConfig>,
) {
    let rx = spawn_receiver(sock, Arc::clone(&handle), shutdown.clone());
    tokio::spawn(process(handle, rx, shutdown, tap));
}

/// Spawn only the socket task; the caller owns the queue. The task never
/// waits on the queue: when it is full, datagrams are dropped and counted.
pub fn spawn_receiver(
    sock: UdpSocket,
    handle: Arc<CaptureHandle>,
    shutdown: watch::Receiver<bool>,
) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>(handle.settings.queue_capacity.max(1));
    tokio::spawn(receive(sock, handle, tx, shutdown));
    rx
}

async fn receive(
    sock: UdpSocket,
    handle: Arc<CaptureHandle>,
    tx: mpsc::Sender<Vec<u8>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        let (len, src) = tokio::select! {
            r = sock.recv_from(&mut buf) => match r {
                Ok(x) => x,
                Err(e) => {
                    tracing::debug!(error = %e, "capture receive failed");
                    continue;
                }
            },
            _ = shutdown.changed() => break,
        };
        if !handle.accepts(src.ip()) {
            inc(&handle.health.counters.rejected_source);
            continue;
        }
        if len > MAX_CAPTURE_DATAGRAM {
            inc(&handle.health.counters.malformed_tzsp);
            continue;
        }
        if handle.queued_bytes.load(Relaxed) + len > QUEUE_MAX_BYTES {
            inc(&handle.health.counters.dropped_backpressure);
            continue;
        }
        match tx.try_send(buf[..len].to_vec()) {
            Ok(()) => {
                handle.queued_bytes.fetch_add(len, Relaxed);
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                inc(&handle.health.counters.dropped_backpressure);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => break,
        }
    }
}

async fn process(
    handle: Arc<CaptureHandle>,
    mut rx: mpsc::Receiver<Vec<u8>>,
    mut shutdown: watch::Receiver<bool>,
    tap: Option<TapConfig>,
) {
    let mut proc = CaptureProcessor::new(handle.outputs(), handle.settings.limits);
    if let Some(t) = tap {
        proc = proc.with_tap(t);
    }
    let mut tick = tokio::time::interval(TICK);
    let mut ticks = 0u32;
    loop {
        tokio::select! {
            d = rx.recv() => match d {
                Some(d) => {
                    handle.queued_bytes.fetch_sub(d.len(), Relaxed);
                    proc.handle(&d, now_ms());
                }
                None => break,
            },
            _ = tick.tick() => {
                let now = now_ms();
                proc.sweep(now);
                ticks = ticks.wrapping_add(1);
                if ticks.is_multiple_of(TABLE_SWEEP_EVERY) {
                    handle.table.sweep(now);
                }
            }
            _ = shutdown.changed() => break,
        }
        handle
            .reassembly_bytes
            .store(proc.reassembly_bytes() as u64, Relaxed);
    }
}
