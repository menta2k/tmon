//! `tmon_dpi_*` Prometheus series (feature 003, contracts/http-api.md).
//! Label sets are fixed: no per-application labels.

use tmon_dpi_client::{DpiState, DpiStatus};

use crate::metrics_capture::{labelled, single};

pub fn render(o: &mut String, s: &DpiStatus) {
    let c = &s.classifier;
    let q = &s.queue;
    let up = u64::from(s.state == DpiState::Running);
    single(
        o,
        "tmon_dpi_up",
        "gauge",
        "1 when the classifier is running.",
        up,
    );
    single(
        o,
        "tmon_dpi_restarts_total",
        "counter",
        "Classifier restarts since tmon started.",
        s.restarts_total,
    );
    labelled(
        o,
        "tmon_dpi_packets_total",
        "counter",
        "Captured packets offered to the classifier, by outcome.",
        "result",
        &[
            ("sent", q.sent),
            ("dropped_queue", q.dropped_queue),
            ("dropped_down", q.dropped_down),
            ("skipped_final", q.skipped_final),
        ],
    );
    labelled(
        o,
        "tmon_dpi_classified_total",
        "counter",
        "Connections classified, by confidence.",
        "confidence",
        &[
            ("confident", c.classified_confident),
            ("guess", c.classified_guess),
        ],
    );
    single(
        o,
        "tmon_dpi_flows_active",
        "gauge",
        "Connections tracked by the classifier.",
        c.flows_active,
    );
    single(
        o,
        "tmon_dpi_memory_bytes",
        "gauge",
        "Classifier memory in use.",
        c.mem_bytes,
    );
    labelled(
        o,
        "tmon_dpi_evicted_total",
        "counter",
        "Classifier connections evicted, by reason.",
        "reason",
        &[
            ("lru", c.evicted_lru),
            ("idle", c.evicted_idle),
            ("memory", c.evicted_memory),
        ],
    );
    single(
        o,
        "tmon_dpi_ipc_invalid_total",
        "counter",
        "Invalid messages received from the classifier.",
        s.ipc_invalid,
    );
}
