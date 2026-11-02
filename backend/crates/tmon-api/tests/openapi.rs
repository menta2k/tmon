mod common;

use axum::http::StatusCode;
use common::*;

/// Every route in `routes/mod.rs` must be documented, and vice versa.
#[tokio::test]
async fn openapi_documents_every_route() {
    let t = app();
    let (s, _, doc) = send(&t.app, get("/api/v1/openapi.json")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(doc["openapi"], "3.1.0");
    let documented: std::collections::BTreeSet<String> =
        doc["paths"].as_object().unwrap().keys().cloned().collect();
    let src = include_str!("../src/routes/mod.rs").to_string() + include_str!("../src/lib.rs");
    let routed: std::collections::BTreeSet<String> = src
        // Tolerates rustfmt putting the path on its own line: `.route(\n    "/x",`.
        .match_indices(".route(")
        .filter_map(|(i, _)| src[i + 7..].trim_start().strip_prefix('"'))
        .map(|rest| rest.split('"').next().unwrap().to_string())
        .filter(|p| p != "/metrics")
        .collect();
    assert_eq!(routed, documented);
}

#[test]
fn feature_002_schemas_are_documented() {
    let doc = tmon_api::openapi::document();
    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["NameSource"]["oneOf"][0]["enum"],
        serde_json::json!(["tls", "quic", "http", "dns", "router", "dpi"])
    );
    let enabled = &schemas["CaptureStatus"]["oneOf"][1]["properties"];
    for k in [
        "listening",
        "address",
        "datagrams",
        "connections",
        "bindings",
        "names",
        "errors",
        "named_share_15m",
    ] {
        assert!(enabled.get(k).is_some(), "CaptureStatus.{k} undocumented");
    }
}

#[test]
fn feature_003_schemas_are_documented() {
    let doc = tmon_api::openapi::document();
    let dpi = &doc["components"]["schemas"]["DpiStatus"]["oneOf"];
    assert_eq!(dpi[0]["properties"]["enabled"]["const"], false);
    for k in [
        "state",
        "engine",
        "queue",
        "classifier",
        "top_apps",
        "restarts_10m",
    ] {
        assert!(
            dpi[1]["properties"].get(k).is_some(),
            "DpiStatus.{k} undocumented"
        );
    }
    let flows = doc["paths"]["/flows"]["get"]["parameters"].to_string();
    assert!(flows.contains("\"app\""));
    let status = doc["paths"]["/status"]["get"]["summary"].as_str().unwrap();
    for code in [
        "dpi_not_found",
        "dpi_incompatible",
        "dpi_failed",
        "dpi_restarting",
        "dpi_dropping",
    ] {
        assert!(status.contains(code), "{code}");
    }
}
