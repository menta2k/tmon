//! Spawn, handshake, heartbeat, back-off restart and give-up (FR-016/017).

mod session;
mod timing;

pub use timing::Timing;

use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, watch};

use crate::health::{DpiHealth, MAX_FAILURES};
use crate::io::Sinks;
use crate::settings::DpiSettings;
use crate::spawn;
use crate::state::DpiState;
use session::Exit;

pub(crate) struct Supervisor {
    pub settings: DpiSettings,
    pub timing: Timing,
    pub sinks: Sinks,
    pub queue: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
    pub stop: watch::Receiver<bool>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl Supervisor {
    fn health(&self) -> &DpiHealth {
        &self.sinks.health
    }

    /// Run until stopped or a terminal state is reached.
    pub async fn run(mut self) {
        let Some(path) = spawn::resolve(&self.settings.binary) else {
            tracing::warn!(binary = %self.settings.binary, "tmon-dpi not found; classification off");
            self.health().set_state(DpiState::NotFound, None);
            return;
        };
        loop {
            let exit = session::run(&mut self, &path).await;
            self.sinks.tap.set_running(false);
            match exit {
                Exit::Stopped => return,
                Exit::Incompatible(v) => {
                    tracing::error!(theirs = v, "tmon-dpi protocol version mismatch");
                    self.health().incompatible(v);
                    return;
                }
                Exit::Failed(error) => {
                    if !self.after_failure(error).await {
                        return;
                    }
                }
            }
        }
    }

    /// Count a failure and wait the back-off; false to stop supervising.
    async fn after_failure(&mut self, error: String) -> bool {
        tracing::warn!(%error, "tmon-dpi failed");
        let n = self.health().failure(error, now_ms());
        if n >= MAX_FAILURES {
            tracing::error!(
                failures = n,
                "tmon-dpi keeps failing; giving up until restart"
            );
            self.health().set_state(DpiState::Failed, None);
            return false;
        }
        self.health().set_state(DpiState::Restarting, None);
        let delay = self.timing.backoff(n);
        tokio::select! {
            _ = tokio::time::sleep(delay) => true,
            _ = self.stop.changed() => false,
        }
    }
}
