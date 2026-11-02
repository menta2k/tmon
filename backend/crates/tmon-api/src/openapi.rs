//! `GET /api/v1/openapi.json` — hand-maintained OpenAPI 3.1 description (contracts/http-api.md).
//! A test (`tests/openapi.rs`) asserts every routed path is documented.

use axum::Json;
use serde_json::{Value, json};

const COMMON: [&str; 11] = [
    "from",
    "to",
    "step",
    "host",
    "category",
    "proto",
    "direction",
    "country",
    "asn",
    "exporter",
    "limit",
];

fn q(name: &str) -> Value {
    json!({"name": name, "in": "query", "required": false, "schema": {"type": "string"}})
}

fn common(extra: &[&str]) -> Vec<Value> {
    COMMON.iter().chain(extra).map(|n| q(n)).collect()
}

fn path_param(name: &str) -> Value {
    json!({"name": name, "in": "path", "required": true, "schema": {"type": "string"}})
}

fn op(summary: &str, params: Vec<Value>, body: bool) -> Value {
    let mut o = json!({
        "summary": summary,
        "parameters": params,
        "responses": {
            "200": {"description": "Envelope `{ok, data, error, meta}`", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Envelope"}}}},
            "400": {"$ref": "#/components/responses/Error"},
            "401": {"$ref": "#/components/responses/Error"}
        }
    });
    if body {
        o["requestBody"] = json!({"required": true, "content": {"application/json": {"schema": {"type": "object"}}}});
    }
    o
}

/// All documented `(path, method)` pairs.
pub fn document() -> Value {
    let id = || vec![path_param("id")];
    json!({
        "openapi": "3.1.0",
        "info": {"title": "tmon API", "version": env!("CARGO_PKG_VERSION")},
        "servers": [{"url": "/api/v1"}],
        "security": [{"session": []}, {"bearer": []}],
        "paths": {
            "/auth/login": {"post": op("Log in; sets the tmon_session cookie (5 attempts/min/IP)", vec![], true)},
            "/auth/logout": {"post": op("Log out", vec![], false)},
            "/health": {"get": op("Liveness and component checks (no auth)", vec![], false)},
            "/status": {"get": op("Exporters, collectors, ingest, storage, enrichment, capture (feature 002: datagrams, connections, names by source, errors, named_share_15m), dpi (feature 003, see DpiStatus), warnings (incl. capture_silent, capture_large_packets_lost, capture_fastpath_suspected, dpi_not_found, dpi_incompatible, dpi_failed, dpi_restarting, dpi_dropping)", vec![], false)},
            "/live": {"get": {"summary": "SSE stream: `event: tick` every 5 s and `event: warning`", "responses": {"200": {"description": "text/event-stream"}}}},
            "/traffic/timeseries": {"get": op("Time series grouped by category|direction|proto|country|exporter", common(&["group_by"]), false)},
            "/traffic/breakdown": {"get": op("Totals by category|proto|direction|country|asn|rule|app (rule rows add `confidence`; app rows add `confident_share`, key \"\" = unclassified, raw ranges only)", common(&["by"]), false)},
            "/top/hosts": {"get": op("Top local hosts", common(&[]), false)},
            "/top/remotes": {"get": op("Top remote endpoints", common(&[]), false)},
            "/top/asns": {"get": op("Top ASNs", common(&[]), false)},
            "/top/countries": {"get": op("Top countries", common(&[]), false)},
            "/top/ports": {"get": op("Top remote ports", common(&[]), false)},
            "/hosts": {"get": op("Local hosts (paged, `q` search)", vec![q("page"), q("limit"), q("q")], false)},
            "/hosts/{ip}": {
                "get": op("Host drill-down (adds `top_apps` for raw ranges)", vec![path_param("ip"), q("from"), q("to")], false),
                "patch": op("Set friendly_name / mac", vec![path_param("ip")], true)
            },
            "/flows": {"get": op("Raw flow search, newest first unless `sort=time|bytes|packets` / `order=desc|asc` (limit ≤ 500); rows carry remote_host_source: tls|quic|http|dns|router|dpi|null, app, app_confident; `app` filter is case-insensitive, an empty `app=` selects unclassified flows", common(&["remote_ip", "port", "page", "app", "sort", "order"]), false)},
            "/flows/export.csv": {"get": {"summary": "CSV export (≤ 100 000 rows, honours `sort`/`order`; last columns remote_host_source, app, app_confident)", "parameters": common(&["remote_ip", "port", "app", "sort", "order"]), "responses": {"200": {"description": "text/csv"}}}},
            "/categories": {"get": op("List categories", vec![], false), "post": op("Create category", vec![], true)},
            "/categories/{id}": {"patch": op("Update category", id(), true), "delete": op("Delete user category (409 for built-ins)", id(), false)},
            "/rules": {"get": op("List rules with hits_24h", vec![], false), "post": op("Create user rule", vec![], true)},
            "/rules/{id}": {"patch": op("Update rule", id(), true), "delete": op("Delete user rule", id(), false)},
            "/rules/reorder": {"post": op("Set user rule order `{ids}`", vec![], true)},
            "/rules/test": {"post": op("Dry-run categorization `{flow}`", vec![], true)},
            "/rules/reapply": {"post": op("Re-apply rules to history `{from?, to?}` → `{job_id}`", vec![], true)},
            "/jobs/{id}": {"get": op("Background job state", id(), false)},
            "/networks": {"get": op("Local networks", vec![], false), "put": op("Replace local networks", vec![], true)},
            "/exporters": {"get": op("Known exporters", vec![], false)},
            "/exporters/{id}": {"patch": op("Update exporter name/sampling_override/count_direction", id(), true)},
            "/settings": {"get": op("Settings (router password never returned; `router.kind` = routeros|openwrt)", vec![], false), "patch": op("Deep-partial settings update", vec![], true)},
            "/settings/router/test": {"post": op("Test router connectivity (RouterOS REST or OpenWrt ubus, per `kind`): fetches DHCP leases", vec![], true)},
            "/settings/export": {"get": op("Export bundle (no secrets)", vec![], false)},
            "/settings/import": {"post": op("Validate fully, then apply bundle atomically", vec![], true)},
            "/applications/catalog": {"get": op("Applications the classifier can report `[{app, category}]`, sorted; `[]` when DPI is off", vec![], false)},
            "/openapi.json": {"get": {"summary": "This document", "responses": {"200": {"description": "OpenAPI 3.1"}}}}
        },
        "components": {
            "securitySchemes": {
                "session": {"type": "apiKey", "in": "cookie", "name": "tmon_session"},
                "bearer": {"type": "http", "scheme": "bearer"}
            },
            "schemas": {
                "Envelope": {
                    "type": "object",
                    "required": ["ok", "data", "error"],
                    "properties": {
                        "ok": {"type": "boolean"},
                        "data": {},
                        "error": {"oneOf": [{"type": "null"}, {"$ref": "#/components/schemas/Error"}]},
                        "meta": {"type": "object", "properties": {
                            "source": {"enum": ["rollup_1m", "rollup_1h", "raw"]}, "sampled": {"type": "boolean"},
                            "page": {"type": "integer"}, "limit": {"type": "integer"}, "total": {"type": "integer"}
                        }}
                    }
                },
                "NameSource": {"description": "How a remote host name was learned (feature 002)", "oneOf": [
                    {"enum": ["tls", "quic", "http", "dns", "router", "dpi"]}, {"type": "null"}
                ]},
                "CaptureStatus": {"description": "`/status.capture`; `{\"enabled\": false}` when capture is off", "oneOf": [
                    {"type": "object", "required": ["enabled"], "properties": {"enabled": {"const": false}}},
                    {"type": "object", "required": ["enabled", "listening", "address", "datagrams", "connections", "bindings", "names", "errors"], "properties": {
                        "enabled": {"const": true},
                        "listening": {"type": "boolean"},
                        "address": {"type": "string"},
                        "error": {"type": ["string", "null"]},
                        "last_datagram_at": {"type": ["string", "null"], "format": "date-time"},
                        "datagrams": {"type": "object", "properties": {
                            "received": {"type": "integer"}, "rejected_source": {"type": "integer"}, "dropped_backpressure": {"type": "integer"},
                            "unsupported": {"type": "integer"}, "malformed": {"type": "integer"}, "max_len_15m": {"type": "integer"}}},
                        "connections": {"type": "object", "properties": {
                            "tracked": {"type": "integer"}, "evicted": {"type": "integer"}, "reassembly_bytes": {"type": "integer"}}},
                        "bindings": {"type": "object", "properties": {"tracked": {"type": "integer"}, "evicted": {"type": "integer"}}},
                        "names": {"type": "object", "properties": {
                            "tls": {"type": "integer"}, "quic": {"type": "integer"}, "http": {"type": "integer"}, "dns": {"type": "integer"}, "ech": {"type": "integer"}}},
                        "errors": {"type": "object", "properties": {
                            "tls_incomplete": {"type": "integer"}, "tls_parse": {"type": "integer"}, "quic_unsupported_version": {"type": "integer"},
                            "quic_decrypt": {"type": "integer"}, "quic_incomplete": {"type": "integer"}, "dns_parse": {"type": "integer"}, "name_invalid": {"type": "integer"}}},
                        "named_share_15m": {"type": ["number", "null"]}
                    }}
                ]},
                "DpiStatus": {"description": "`/status.dpi` (feature 003); `{\"enabled\": false}` when DPI is off", "oneOf": [
                    {"type": "object", "required": ["enabled"], "properties": {"enabled": {"const": false}}},
                    {"type": "object", "required": ["enabled", "state", "queue", "classifier", "top_apps"], "properties": {
                        "enabled": {"const": true},
                        "state": {"enum": ["disabled", "not_found", "starting", "running", "restarting", "failed", "incompatible"]},
                        "engine": {"type": ["string", "null"]}, "impl_version": {"type": ["string", "null"]}, "proto_version": {"type": ["integer", "null"]},
                        "uptime_secs": {"type": ["integer", "null"]}, "restarts_10m": {"type": "integer"}, "restarts_total": {"type": "integer"},
                        "error": {"type": ["string", "null"]},
                        "queue": {"type": "object", "properties": {
                            "sent": {"type": "integer"}, "dropped_queue": {"type": "integer"}, "dropped_down": {"type": "integer"},
                            "skipped_final": {"type": "integer"}, "drop_share_15m": {"type": "number"}}},
                        "classifier": {"type": "object", "properties": {
                            "packets_in": {"type": "integer"}, "flows_active": {"type": "integer"}, "classified_confident": {"type": "integer"},
                            "classified_guess": {"type": "integer"}, "evicted": {"type": "integer"}, "mem_bytes": {"type": "integer"}, "mem_limit_bytes": {"type": "integer"}}},
                        "top_apps": {"type": "array", "items": {"type": "object", "properties": {"app": {"type": "string"}, "flows": {"type": "integer"}}}}
                    }}
                ]},
                "Error": {"type": "object", "required": ["code", "message"], "properties": {
                    "code": {"enum": ["unauthorized", "validation_failed", "not_found", "conflict", "unsupported_filter_range", "rate_limited", "internal"]},
                    "message": {"type": "string"},
                    "fields": {"type": "object", "additionalProperties": {"type": "string"}}
                }}
            },
            "responses": {"Error": {"description": "Error envelope", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Envelope"}}}}}
        }
    })
}

pub async fn openapi() -> Json<Value> {
    Json(document())
}
