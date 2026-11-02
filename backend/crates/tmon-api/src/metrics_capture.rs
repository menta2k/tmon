//! `tmon_capture_*` Prometheus series (feature 002, contracts/http-api.md).

use std::fmt::Write;
use tmon_capture::CaptureStats;

pub(crate) fn family(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {kind}");
}

/// One family with a fixed, bounded label set.
pub(crate) fn labelled(
    o: &mut String,
    name: &str,
    kind: &str,
    help: &str,
    label: &str,
    series: &[(&str, u64)],
) {
    family(o, name, kind, help);
    for (v, n) in series {
        let _ = writeln!(o, "{name}{{{label}=\"{v}\"}} {n}");
    }
}

pub(crate) fn single(o: &mut String, name: &str, kind: &str, help: &str, n: u64) {
    family(o, name, kind, help);
    let _ = writeln!(o, "{name} {n}");
}

pub fn render(o: &mut String, s: &CaptureStats) {
    let c = &s.counters;
    single(
        o,
        "tmon_capture_datagrams_total",
        "counter",
        "Capture datagrams accepted.",
        c.datagrams_received,
    );
    single(
        o,
        "tmon_capture_rejected_total",
        "counter",
        "Capture datagrams from senders not allowed.",
        c.rejected_source,
    );
    labelled(
        o,
        "tmon_capture_dropped_total",
        "counter",
        "Capture datagrams dropped.",
        "reason",
        &[("backpressure", c.dropped_backpressure)],
    );
    labelled(
        o,
        "tmon_capture_unsupported_total",
        "counter",
        "Capture datagrams of an unsupported kind.",
        "kind",
        &[
            ("tzsp", c.unsupported_tzsp),
            ("encap", c.unsupported_encap),
            ("l3", c.unsupported_l3),
            ("fragment", c.unsupported_fragment),
        ],
    );
    labelled(
        o,
        "tmon_capture_malformed_total",
        "counter",
        "Capture datagrams that failed to parse.",
        "layer",
        &[("tzsp", c.malformed_tzsp), ("frame", c.malformed_frame)],
    );
    labelled(
        o,
        "tmon_capture_names_total",
        "counter",
        "Names learned by source.",
        "source",
        &[
            ("tls", c.tls_named),
            ("quic", c.quic_named),
            ("http", c.http_named),
            ("dns", c.dns_bindings),
        ],
    );
    labelled(
        o,
        "tmon_capture_errors_total",
        "counter",
        "Naming failures by kind.",
        "kind",
        &[
            ("tls_incomplete", c.tls_incomplete),
            ("tls_parse", c.tls_parse_error),
            ("quic_version", c.quic_unsupported_version),
            ("quic_decrypt", c.quic_decrypt_error),
            ("quic_incomplete", c.quic_incomplete),
            ("dns_parse", c.dns_parse_error),
            ("name_invalid", c.name_invalid),
        ],
    );
    labelled(
        o,
        "tmon_capture_evicted_total",
        "counter",
        "Table evictions.",
        "what",
        &[
            ("conn_lru", c.conn_evicted_lru),
            ("conn_ttl", c.conn_evicted_ttl),
            ("conn_bytes", c.conn_evicted_bytes),
            ("binding", c.bindings_evicted),
        ],
    );
    single(
        o,
        "tmon_capture_connections",
        "gauge",
        "Named connections tracked.",
        s.conns_tracked,
    );
    single(
        o,
        "tmon_capture_bindings",
        "gauge",
        "DNS device bindings tracked.",
        s.bindings_tracked,
    );
    single(
        o,
        "tmon_capture_reassembly_bytes",
        "gauge",
        "Bytes buffered for reassembly.",
        s.reassembly_bytes,
    );
    let miss = c
        .lookups
        .saturating_sub(c.lookup_hits_conn + c.lookup_hits_dns);
    labelled(
        o,
        "tmon_capture_lookups_total",
        "counter",
        "Flow name lookups by result.",
        "result",
        &[
            ("conn", c.lookup_hits_conn),
            ("dns", c.lookup_hits_dns),
            ("miss", miss),
        ],
    );
}
