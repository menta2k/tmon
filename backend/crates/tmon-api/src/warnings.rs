//! Operational warnings shown as banners (FR-019) and pushed over SSE.

use tmon_capture::health::CaptureWarning;
pub use tmon_collector::health::Warning;
use tmon_collector::health::stall_warnings;

use crate::state::AppState;

/// All current warnings, sorted by code for stable diffs.
pub fn current(s: &AppState) -> Vec<Warning> {
    let now = s.now_ms();
    let mut out = stall_warnings(
        &s.registry.snapshot(now, s.stall_after_ms),
        now,
        s.stall_after_ms,
    );
    out.extend(s.listeners.load().iter().filter(|l| !l.listening).map(|l| {
        Warning::new(
            format!("collector_down:{}", l.port),
            format!(
                "Collector {} cannot listen on {}: {}",
                format!("{:?}", l.proto).to_lowercase(),
                l.address,
                l.error.as_deref().unwrap_or("unknown error")
            ),
        )
    }));
    if !s.pipeline.deps().geo.status().loaded {
        out.push(Warning::new(
            "geoip_missing",
            "GeoIP databases not loaded — run `tmon geoip-update` (country/ASN columns stay empty)",
        ));
    }
    let router = s.router.load();
    if let (true, Some(err)) = (router.enabled, &router.error) {
        out.push(Warning::new(
            "router_unreachable",
            format!("Router integration failing: {err} (flow collection continues)"),
        ));
    }
    if let Some(h) = &s.capture {
        out.extend(capture_warnings(h, now));
    }
    if let Some(h) = &s.dpi {
        out.extend(crate::routes::dpi::warnings(h, now));
    }
    out.extend(s.external_warnings.load().iter().cloned());
    out.sort_by(|a, b| a.code.cmp(&b.code));
    out
}

/// Capture health (feature 002, FR-019..FR-021); a bind failure uses the
/// existing `collector_down:<port>` shape.
fn capture_warnings(h: &tmon_capture::CaptureHandle, now_ms: i64) -> Vec<Warning> {
    let error = h.state().error;
    h.warnings(now_ms)
        .into_iter()
        .map(|w| {
            let message = match (&w, &error) {
                (CaptureWarning::ListenerDown { .. }, Some(e)) => format!("{} ({e})", w.message()),
                _ => w.message(),
            };
            Warning::new(w.code(), message)
        })
        .collect()
}
