//! Socket I/O with a running classifier: the writer drains the tap queue,
//! the reader applies classifier messages (all input is untrusted).

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};
use tmon_core::app::AppInfo;
use tmon_dpi_proto::{
    Classification, DecodeError, FromClassifier, MAX_CLASSIFICATIONS_PER_SEC, MAX_MESSAGE,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::ChildStderr;
use tokio::sync::mpsc;
use tokio_seqpacket::UnixSeqpacket;

use crate::convert::from_wire;
use crate::health::DpiHealth;
use crate::pending::PendingApps;
use crate::table::{AppTable, Insert};
use crate::tap::DpiTap;

/// Where classifier answers go.
pub struct Sinks {
    pub table: Arc<AppTable>,
    pub tap: Arc<DpiTap>,
    pub pending: Arc<PendingApps>,
    pub health: Arc<DpiHealth>,
}

/// Forward queued packets until the queue or the socket closes. A full
/// socket makes this task wait; the tap then drops at the queue (FR-018).
pub async fn writer(
    sock: Arc<UnixSeqpacket>,
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>>,
    tap: Arc<DpiTap>,
) {
    let mut rx = rx.lock().await;
    // Packets queued while no classifier was running are stale.
    while let Ok(m) = rx.try_recv() {
        tap.dequeued(m.len());
    }
    while let Some(m) = rx.recv().await {
        tap.dequeued(m.len());
        if sock.send(&m).await.is_err() {
            return;
        }
    }
}

/// Receive one message; `None` on EOF or socket error.
pub async fn recv(sock: &UnixSeqpacket, buf: &mut [u8]) -> Option<usize> {
    match sock.recv(buf).await {
        Ok(0) | Err(_) => None,
        Ok(n) => Some(n),
    }
}

pub fn recv_buffer() -> Vec<u8> {
    // One byte more than allowed, so oversized messages are detectable.
    vec![0u8; MAX_MESSAGE + 1]
}

/// Most non-Classification messages (Stats, Pong, catalog, unknown types)
/// accepted per second; the sidecar sends about one. Beyond it each message
/// counts as invalid, so a flooding classifier is restarted.
const MAX_CONTROL_PER_SEC: u32 = 50;

/// Per-second budgets for Classification and control messages (contracts
/// "Limits").
pub struct RateLimit {
    window: Instant,
    count: u32,
    control: u32,
}

impl Default for RateLimit {
    fn default() -> Self {
        Self {
            window: Instant::now(),
            count: 0,
            control: 0,
        }
    }
}

impl RateLimit {
    fn tick(&mut self) {
        if self.window.elapsed() >= Duration::from_secs(1) {
            self.window = Instant::now();
            self.count = 0;
            self.control = 0;
        }
    }

    fn allow(&mut self) -> bool {
        self.tick();
        self.count = self.count.saturating_add(1);
        self.count <= MAX_CLASSIFICATIONS_PER_SEC
    }

    fn allow_control(&mut self) -> bool {
        self.tick();
        self.control = self.control.saturating_add(1);
        self.control <= MAX_CONTROL_PER_SEC
    }
}

/// Outcome of handling one received message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    Ok,
    Heartbeat,
    Invalid,
}

/// Apply one classifier message.
pub fn handle(msg: &[u8], s: &Sinks, rate: &mut RateLimit, now_ms: i64) -> Handled {
    let decoded = FromClassifier::decode(msg);
    let control = match &decoded {
        Ok(m) => !matches!(m, FromClassifier::Classification(_)),
        Err(e) => matches!(e, DecodeError::UnknownType(_)),
    };
    if control && !rate.allow_control() {
        s.health.io.ipc_invalid.fetch_add(1, Relaxed);
        return Handled::Invalid;
    }
    match decoded {
        Ok(FromClassifier::Classification(c)) => {
            if rate.allow() {
                classification(&c, s, now_ms);
            } else {
                s.health.io.classifications_dropped.fetch_add(1, Relaxed);
            }
            Handled::Ok
        }
        Ok(FromClassifier::Stats(st)) => {
            s.health.stats(st, now_ms);
            Handled::Heartbeat
        }
        Ok(FromClassifier::HelloCatalog(c)) => {
            s.health.add_catalog(&c);
            Handled::Ok
        }
        // A second Hello or a Pong carries nothing to apply.
        Ok(FromClassifier::Hello(_) | FromClassifier::Pong(_)) => Handled::Ok,
        // Unknown types are ignored for forward compatibility.
        Err(DecodeError::UnknownType(_)) => Handled::Ok,
        Err(_) => {
            s.health.io.ipc_invalid.fetch_add(1, Relaxed);
            Handled::Invalid
        }
    }
}

fn classification(c: &Classification, s: &Sinks, now_ms: i64) {
    s.health.io.classifications.fetch_add(1, Relaxed);
    let key = from_wire(&c.key);
    if c.final_ {
        s.tap.mark_final(key);
    }
    let info = match AppInfo::new(
        &c.app,
        &c.category,
        c.confident,
        c.host.as_deref(),
        c.final_,
    ) {
        Ok(i) => Arc::new(i),
        Err(_) => {
            s.health.io.app_invalid.fetch_add(1, Relaxed);
            return;
        }
    };
    if s.table.insert(key, Arc::clone(&info), now_ms) == Insert::Stored {
        s.pending.push(key, info);
    }
}

/// Most classifier log lines forwarded per second.
const LOG_LINES_PER_SEC: u32 = 20;

/// Longest stderr line kept; longer lines are split (bounded memory).
const MAX_LOG_LINE: u64 = 512;

/// Forward classifier stderr to tmon's log at info level, rate-limited (it
/// is quiet in normal operation: startup, sandbox steps, fatal errors).
pub async fn forward_stderr(stderr: ChildStderr) {
    let mut r = BufReader::new(stderr);
    let mut line = Vec::with_capacity(MAX_LOG_LINE as usize);
    let mut window = Instant::now();
    let mut n = 0u32;
    loop {
        line.clear();
        match (&mut r)
            .take(MAX_LOG_LINE)
            .read_until(b'\n', &mut line)
            .await
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if window.elapsed() >= Duration::from_secs(1) {
            window = Instant::now();
            n = 0;
        }
        n = n.saturating_add(1);
        if n <= LOG_LINES_PER_SEC {
            let text = String::from_utf8_lossy(&line);
            tracing::info!("{}", text.trim_end());
        }
    }
}
