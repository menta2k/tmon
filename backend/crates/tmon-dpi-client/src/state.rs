//! Supervisor state (data-model.md §5).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DpiState {
    Disabled,
    NotFound,
    Starting,
    Running,
    Restarting,
    Failed,
    Incompatible,
}

impl DpiState {
    pub fn as_str(self) -> &'static str {
        match self {
            DpiState::Disabled => "disabled",
            DpiState::NotFound => "not_found",
            DpiState::Starting => "starting",
            DpiState::Running => "running",
            DpiState::Restarting => "restarting",
            DpiState::Failed => "failed",
            DpiState::Incompatible => "incompatible",
        }
    }

    /// Terminal states: the supervisor stops trying until tmon restarts.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            DpiState::NotFound | DpiState::Failed | DpiState::Incompatible | DpiState::Disabled
        )
    }
}

/// Health warnings with remediation hints (FR-020).
#[derive(Debug, Clone, PartialEq)]
pub enum DpiWarning {
    NotFound { binary: String },
    Incompatible { theirs: u16, ours: u16 },
    Failed { error: String },
    Restarting { count: usize },
    Dropping { share: f64 },
}

impl DpiWarning {
    pub fn code(&self) -> &'static str {
        match self {
            DpiWarning::NotFound { .. } => "dpi_not_found",
            DpiWarning::Incompatible { .. } => "dpi_incompatible",
            DpiWarning::Failed { .. } => "dpi_failed",
            DpiWarning::Restarting { .. } => "dpi_restarting",
            DpiWarning::Dropping { .. } => "dpi_dropping",
        }
    }

    pub fn message(&self) -> String {
        match self {
            DpiWarning::NotFound { binary } => format!(
                "Application classification is enabled but `tmon-dpi` was not found at {binary}. \
                 Install the tmon-dpi package or use the -dpi image."
            ),
            DpiWarning::Incompatible { theirs, ours } => format!(
                "tmon-dpi speaks protocol v{theirs}, tmon needs v{ours}. Install matching versions."
            ),
            DpiWarning::Failed { error } => format!(
                "Application classification stopped after repeated crashes ({error}). \
                 Restart tmon to retry; see logs."
            ),
            DpiWarning::Restarting { count } => {
                format!("The classifier restarted {count} times in 10 min.")
            }
            DpiWarning::Dropping { share } => format!(
                "The classifier cannot keep up; {:.1}% of packets were skipped. \
                 Raise dpi.queue_capacity or reduce capture.",
                share * 100.0
            ),
        }
    }
}
