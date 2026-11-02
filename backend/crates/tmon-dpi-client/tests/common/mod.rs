#![allow(dead_code)]
//! Shared helpers: the `fake_dpi` example as classifier, fast timings.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tmon_core::naming::ConnKey;
use tmon_dpi_client::{DpiHandle, DpiSettings, Timing};
use tokio::sync::watch;

/// `TMON_FAKE_DPI` (set by `just coverage`) or
/// `target/<profile>/examples/fake_dpi` (built by `cargo test`).
pub fn fake_dpi() -> PathBuf {
    if let Some(p) = std::env::var_os("TMON_FAKE_DPI") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap();
    let p = dir.join("examples").join("fake_dpi");
    assert!(p.exists(), "build the example first: {}", p.display());
    p
}

pub fn settings(extra: &[&str]) -> DpiSettings {
    DpiSettings {
        binary: fake_dpi().display().to_string(),
        extra_args: extra.iter().map(|s| s.to_string()).collect(),
        ..DpiSettings::default()
    }
}

pub fn fast() -> Timing {
    Timing {
        hello_timeout: Duration::from_millis(1_500),
        heartbeat_timeout: Duration::from_millis(600),
        backoff_base: Duration::from_millis(50),
        backoff_max: Duration::from_millis(200),
        tick: Duration::from_millis(50),
        kill_grace: Duration::from_millis(300),
        invalid_window: Duration::from_secs(10),
        invalid_limit: 100,
    }
}

pub struct Running {
    pub handle: Arc<DpiHandle>,
    pub stop: watch::Sender<bool>,
    pub task: tokio::task::JoinHandle<()>,
}

pub fn start(settings: DpiSettings, timing: Timing) -> Running {
    let (stop, rx) = watch::channel(false);
    let (handle, task) = tmon_dpi_client::start(settings, timing, rx);
    Running { handle, stop, task }
}

impl Running {
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}

/// Poll `cond` every 20 ms until true or `limit` passes.
pub async fn wait_for(limit: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let end = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < end {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cond()
}

pub fn key(port: u16) -> ConnKey {
    ConnKey {
        lan_ip: "192.168.88.10".parse().unwrap(),
        lan_port: 50_000,
        remote_ip: "203.0.113.5".parse().unwrap(),
        remote_port: port,
        proto: 17,
    }
}

/// A minimal IPv4/UDP packet for `key` (the fake classifier only reads the key).
pub fn l3(k: &ConnKey) -> Vec<u8> {
    use tmon_capture::fixtures::udp_ip;
    let a = std::net::SocketAddr::new(k.lan_ip, k.lan_port);
    let b = std::net::SocketAddr::new(k.remote_ip, k.remote_port);
    udp_ip(a, b, b"payload")
}

pub fn now() -> i64 {
    tmon_dpi_client::now_ms()
}
