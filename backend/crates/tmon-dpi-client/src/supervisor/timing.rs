//! Supervision timing (research R7); shortened in tests.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// Hello must arrive within this after spawn.
    pub hello_timeout: Duration,
    /// No Stats (heartbeat) for this long → restart.
    pub heartbeat_timeout: Duration,
    /// First restart delay; doubles per recent failure.
    pub backoff_base: Duration,
    pub backoff_max: Duration,
    /// Housekeeping interval (heartbeat check, drop-share sampling).
    pub tick: Duration,
    /// SIGKILL after this when stopping.
    pub kill_grace: Duration,
    /// More than `invalid_limit` invalid messages within `invalid_window` → restart.
    pub invalid_window: Duration,
    pub invalid_limit: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            hello_timeout: Duration::from_secs(5),
            heartbeat_timeout: Duration::from_secs(6),
            backoff_base: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
            tick: Duration::from_secs(1),
            kill_grace: Duration::from_secs(3),
            invalid_window: Duration::from_secs(10),
            invalid_limit: 100,
        }
    }
}

impl Timing {
    /// Delay before the restart that follows the `failures`-th recent failure:
    /// base, 2×base, 4×base … capped at `backoff_max`.
    pub fn backoff(&self, failures: usize) -> Duration {
        let shift = failures.saturating_sub(1).min(16) as u32;
        self.backoff_base
            .saturating_mul(1u32 << shift)
            .min(self.backoff_max)
    }
}
