//! How sure a categorization is, judged from the evidence its rule matched on.

use serde_json::json;
use tmon_core::rules::{Confidence, RuleMatch, load_builtin};

fn m(v: serde_json::Value) -> RuleMatch {
    RuleMatch::from_json(&v).unwrap()
}

#[test]
fn hostname_rules_are_detected() {
    assert_eq!(
        m(json!({"hostname": ["*.googlevideo.com"]})).confidence(),
        Confidence::Detected
    );
    assert_eq!(
        m(json!({"hostname": ["*.x.com"], "remote_port": [443]})).confidence(),
        Confidence::Detected
    );
}

#[test]
fn address_based_rules_are_inferred() {
    assert_eq!(m(json!({"asn": [2906]})).confidence(), Confidence::Inferred);
    assert_eq!(
        m(json!({"remote_cidr": ["17.0.0.0/8"]})).confidence(),
        Confidence::Inferred
    );
    assert_eq!(
        m(json!({"local_host": ["192.168.88.20"]})).confidence(),
        Confidence::Inferred
    );
    assert_eq!(
        m(json!({"country": ["CN"], "proto": [6]})).confidence(),
        Confidence::Inferred
    );
}

#[test]
fn port_and_protocol_rules_are_guessed() {
    assert_eq!(
        m(json!({"remote_port": [443]})).confidence(),
        Confidence::Guessed
    );
    assert_eq!(
        m(json!({"proto": [17], "remote_port": ["27000-27100"]})).confidence(),
        Confidence::Guessed
    );
    assert_eq!(
        m(json!({"direction": ["internal"], "proto": [2]})).confidence(),
        Confidence::Guessed
    );
}

#[test]
fn labels_are_stable_api_strings() {
    let all = [
        Confidence::Detected,
        Confidence::Inferred,
        Confidence::Guessed,
        Confidence::None,
    ];
    let labels: Vec<&str> = all.iter().map(Confidence::as_str).collect();
    assert_eq!(labels, ["detected", "inferred", "guessed", "none"]);
}

#[test]
fn web_catch_all_is_guessed_and_youtube_is_detected() {
    let b = load_builtin().unwrap();
    let by_name = |n: &str| {
        b.rules
            .iter()
            .find(|r| r.name == n)
            .unwrap()
            .match_
            .confidence()
    };
    assert_eq!(by_name("HTTPS / HTTP / QUIC"), Confidence::Guessed);
    assert_eq!(by_name("YouTube video"), Confidence::Detected);
    assert_eq!(by_name("Netflix (AS2906)"), Confidence::Inferred);
}
