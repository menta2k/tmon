//! Application classification seam (feature 003): what application a
//! connection carries, as reported by an external classifier (nDPI sidecar).

use std::sync::Arc;

use crate::naming::{ConnKey, normalize_host};

/// Longest application or category name.
pub const MAX_APP_NAME: usize = 48;

/// Carriers that say nothing about the application; never stored as `app`
/// (FR-006). Compared case-insensitively.
pub const GENERIC_CARRIERS: [&str; 6] = ["TLS", "QUIC", "HTTP", "DNS", "Unknown", "HTTP_Proxy"];

/// Why classifier output was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AppError {
    #[error("invalid application name")]
    InvalidApp,
    #[error("generic carrier is not an application")]
    GenericCarrier,
    #[error("invalid category name")]
    InvalidCategory,
}

/// 1–48 printable ASCII characters (0x20–0x7E), not blank.
/// Markup and quoting characters: no classifier name contains them, so a
/// name that does is rejected (defense in depth for every sink).
const MARKUP: &[u8] = b"<>\"'&`\\";

pub fn valid_app_name(s: &str) -> bool {
    (1..=MAX_APP_NAME).contains(&s.len())
        && s.bytes()
            .all(|b| (0x20..=0x7e).contains(&b) && !MARKUP.contains(&b))
        && !s.trim().is_empty()
}

pub fn is_generic_carrier(app: &str) -> bool {
    GENERIC_CARRIERS.iter().any(|g| g.eq_ignore_ascii_case(app))
}

/// One classification result. Immutable once built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    pub app: Arc<str>,
    /// Classifier's own category (e.g. `VPN`); may be empty.
    pub category: Arc<str>,
    /// Confident detection (true) or a guess (false).
    pub confident: bool,
    pub host: Option<Arc<str>>,
    /// The classifier will not refine this connection further.
    pub final_: bool,
}

impl AppInfo {
    /// Validate classifier output. An invalid host is dropped, not an error.
    pub fn new(
        app: &str,
        category: &str,
        confident: bool,
        host: Option<&str>,
        final_: bool,
    ) -> Result<Self, AppError> {
        if !valid_app_name(app) {
            return Err(AppError::InvalidApp);
        }
        if is_generic_carrier(app) {
            return Err(AppError::GenericCarrier);
        }
        if !category.is_empty() && !valid_app_name(category) {
            return Err(AppError::InvalidCategory);
        }
        Ok(Self {
            app: Arc::from(app),
            category: Arc::from(category),
            confident,
            host: host.and_then(normalize_host).map(Arc::from),
            final_,
        })
    }

    /// Should `self` replace `old` for the same connection? A guess never
    /// replaces a confident result (FR-006); the same answer is no news.
    pub fn better_than(&self, old: &AppInfo) -> bool {
        if old.confident && !self.confident {
            return false;
        }
        self.app != old.app
            || self.confident != old.confident
            || (self.host.is_some() && self.host != old.host)
            || (self.final_ && !old.final_)
    }
}

/// Replaceable classification step (FR-027).
pub trait ConnectionClassifier: Send + Sync {
    fn classify(&self, key: &ConnKey, now_ms: i64) -> Option<Arc<AppInfo>>;
}

#[cfg(test)]
mod name_tests {
    use super::valid_app_name;

    #[test]
    fn app_names_reject_markup_and_control_bytes() {
        for ok in [
            "BitTorrent",
            "Source_Engine",
            "Apple iCloud",
            "Google-Meet",
            "Cybersec/x",
        ] {
            assert!(valid_app_name(ok), "{ok}");
        }
        for bad in [
            "",
            " ",
            "<script>",
            "a\"b",
            "a'b",
            "a&b",
            "a`b",
            "a\\b",
            "a\nb",
            &"x".repeat(49),
        ] {
            assert!(!valid_app_name(bad), "{bad:?}");
        }
    }
}
