//! `GET /metrics` — Prometheus text exposition with bounded label sets (FR-019).

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::collections::BTreeMap;
use std::fmt::Write;

use crate::state::AppState;

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn family(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {kind}");
}

/// Render all metrics (pure; `categories` maps id → name).
pub fn render(s: &AppState, categories: &BTreeMap<u16, String>, storage_bytes: u64) -> String {
    let mut o = String::new();
    let exporters = s.registry.all();
    family(
        &mut o,
        "tmon_flows_received_total",
        "counter",
        "Flow records decoded per exporter and protocol.",
    );
    for e in &exporters {
        let c = e.counters.snapshot();
        let _ = writeln!(
            o,
            "tmon_flows_received_total{{exporter=\"{}\",proto=\"{}\"}} {}",
            escape(&e.address.to_string()),
            e.protocol().as_str(),
            c.decoded
        );
    }
    family(
        &mut o,
        "tmon_flows_dropped_total",
        "counter",
        "Flow records or datagrams dropped, by reason.",
    );
    for (reason, n) in s.globals.snapshot() {
        let _ = writeln!(
            o,
            "tmon_flows_dropped_total{{reason=\"{}\"}} {n}",
            reason.as_str()
        );
    }
    let sum = |f: fn(&tmon_collector::CountersSnapshot) -> u64| {
        exporters
            .iter()
            .map(|e| f(&e.counters.snapshot()))
            .sum::<u64>()
    };
    family(
        &mut o,
        "tmon_decode_errors_total",
        "counter",
        "Datagrams that failed to parse.",
    );
    let _ = writeln!(o, "tmon_decode_errors_total {}", sum(|c| c.parse_errors));
    family(
        &mut o,
        "tmon_template_missing_total",
        "counter",
        "Data sets received before their template.",
    );
    let _ = writeln!(
        o,
        "tmon_template_missing_total {}",
        sum(|c| c.template_missing)
    );
    family(
        &mut o,
        "tmon_bytes_total",
        "counter",
        "Bytes ingested by category and direction.",
    );
    for ((cat, dir), bytes) in s.live.totals() {
        let name = categories
            .get(&cat)
            .cloned()
            .unwrap_or_else(|| cat.to_string());
        let _ = writeln!(
            o,
            "tmon_bytes_total{{category=\"{}\",direction=\"{}\"}} {bytes}",
            escape(&name),
            dir.as_str()
        );
    }
    family(
        &mut o,
        "tmon_storage_bytes",
        "gauge",
        "Bytes used by the database and raw Parquet files.",
    );
    let _ = writeln!(o, "tmon_storage_bytes {storage_bytes}");
    if let Some(h) = &s.capture {
        crate::metrics_capture::render(&mut o, &h.stats(s.now_ms()));
    }
    if let Some(h) = &s.dpi {
        crate::metrics_dpi::render(&mut o, &h.status(s.now_ms()));
    }
    o
}

/// Open when no `api_token` is configured; otherwise the token or a session is required.
pub async fn metrics(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if !(s.auth.is_authorized(&headers, s.now_ms()) || s.auth.token_ok(&headers)) {
        return crate::envelope::ApiError::Unauthorized.into_response();
    }
    let data = s
        .blocking(|st| {
            let cats = tmon_store::duck::repo::categories::list(&st.conn()?)?
                .into_iter()
                .map(|c| (c.id, c.name))
                .collect();
            Ok((cats, tmon_store::duck::retention::disk_bytes(st)?))
        })
        .await;
    match data {
        Ok((cats, bytes)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            render(&s, &cats, bytes),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}
