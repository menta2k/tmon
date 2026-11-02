//! One classifier lifetime: spawn → Hello → serve until exit or failure.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tmon_dpi_proto::{FromClassifier, Hello, PROTO_VERSION};
use tokio::process::Child;
use tokio_seqpacket::UnixSeqpacket;

use super::{Supervisor, now_ms};
use crate::io::{self, Handled, RateLimit};
use crate::spawn;

#[derive(Debug)]
pub(super) enum Exit {
    Stopped,
    Incompatible(u16),
    Failed(String),
}

pub(super) async fn run(sup: &mut Supervisor, path: &Path) -> Exit {
    let spawned = match spawn::spawn(path, &sup.settings) {
        Ok(s) => s,
        Err(e) => return Exit::Failed(format!("spawn failed: {e}")),
    };
    let mut child = spawned.child;
    if let Some(err) = child.stderr.take() {
        tokio::spawn(io::forward_stderr(err));
    }
    let sock = Arc::new(spawned.sock);
    let exit = match handshake(sup, &sock).await {
        Ok(hello) => serve(sup, &sock, &mut child, &hello).await,
        Err(e) => e,
    };
    stop_child(&mut child, sock, sup.timing.kill_grace).await;
    exit
}

async fn handshake(sup: &mut Supervisor, sock: &UnixSeqpacket) -> Result<Hello, Exit> {
    let mut buf = io::recv_buffer();
    let first = tokio::select! {
        r = tokio::time::timeout(sup.timing.hello_timeout, io::recv(sock, &mut buf)) => r,
        _ = sup.stop.changed() => return Err(Exit::Stopped),
    };
    let n = match first {
        Err(_) => return Err(Exit::Failed("no Hello within the start timeout".into())),
        Ok(None) => {
            return Err(Exit::Failed(
                "classifier closed the socket before Hello".into(),
            ));
        }
        Ok(Some(n)) => n,
    };
    match FromClassifier::decode(&buf[..n]) {
        Ok(FromClassifier::Hello(h)) if h.proto_version == PROTO_VERSION => Ok(h),
        Ok(FromClassifier::Hello(h)) => Err(Exit::Incompatible(h.proto_version)),
        _ => Err(Exit::Failed("first message was not a valid Hello".into())),
    }
}

async fn serve(
    sup: &mut Supervisor,
    sock: &Arc<UnixSeqpacket>,
    child: &mut Child,
    hello: &Hello,
) -> Exit {
    let now = now_ms();
    sup.sinks.health.running(hello, now);
    sup.sinks.health.stats(Default::default(), now);
    sup.sinks.tap.reset();
    sup.sinks.tap.set_running(true);
    tracing::info!(engine = %hello.engine, version = %hello.impl_version, "tmon-dpi running");
    let writer = tokio::spawn(io::writer(
        Arc::clone(sock),
        Arc::clone(&sup.queue),
        Arc::clone(&sup.sinks.tap),
    ));
    let exit = serve_loop(sup, sock, child).await;
    // Stop queueing now, not after the (up to kill_grace) child shutdown.
    sup.sinks.tap.set_running(false);
    writer.abort();
    exit
}

async fn serve_loop(sup: &mut Supervisor, sock: &UnixSeqpacket, child: &mut Child) -> Exit {
    let mut buf = io::recv_buffer();
    let mut rate = RateLimit::default();
    let mut tick = tokio::time::interval(sup.timing.tick);
    let mut invalid = (Instant::now(), 0u64);
    loop {
        tokio::select! {
            r = io::recv(sock, &mut buf) => {
                let Some(n) = r else { return closed(child).await };
                if io::handle(&buf[..n], &sup.sinks, &mut rate, now_ms()) == Handled::Invalid {
                    invalid.1 += 1;
                    if invalid.1 > sup.timing.invalid_limit {
                        return Exit::Failed("too many invalid messages".into());
                    }
                }
            }
            st = child.wait() => return exited(st),
            _ = tick.tick() => {
                if invalid.0.elapsed() >= sup.timing.invalid_window {
                    invalid = (Instant::now(), 0);
                }
                if let Some(e) = housekeeping(sup) { return e; }
            }
            _ = sup.stop.changed() => return Exit::Stopped,
        }
    }
}

fn exited(st: std::io::Result<std::process::ExitStatus>) -> Exit {
    Exit::Failed(match st {
        Ok(s) => format!("classifier exited ({s})"),
        Err(e) => format!("classifier wait failed: {e}"),
    })
}

/// The socket closed: usually the process is exiting; report its status.
async fn closed(child: &mut Child) -> Exit {
    match tokio::time::timeout(std::time::Duration::from_millis(500), child.wait()).await {
        Ok(st) => exited(st),
        Err(_) => Exit::Failed("classifier closed the socket".into()),
    }
}

/// Heartbeat check and drop-share sampling.
fn housekeeping(sup: &Supervisor) -> Option<Exit> {
    let now = now_ms();
    let h = &sup.sinks.health;
    h.sample(sup.sinks.tap.counters.values(), now);
    let last = h.last_stats_ms().unwrap_or(now);
    let limit = sup.timing.heartbeat_timeout.as_millis() as i64;
    (now - last > limit).then(|| Exit::Failed("no heartbeat from the classifier".into()))
}

/// Close our socket end (the classifier exits on EOF), then SIGKILL after
/// the grace period. Always reaps the child.
async fn stop_child(child: &mut Child, sock: Arc<UnixSeqpacket>, grace: std::time::Duration) {
    let _ = sock.shutdown(std::net::Shutdown::Both);
    drop(sock);
    if tokio::time::timeout(grace, child.wait()).await.is_err() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}
