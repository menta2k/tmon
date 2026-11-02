//! Application rule fields (feature 003, data-model.md §3).

mod common;

use common::flow;
use serde_json::json;
use std::collections::HashMap;
use tmon_core::rules::{Confidence, Origin, Rule, RuleMatch, RuleSet};
use tmon_core::{AppInfo, Categorizer, FlowRecord};

fn rule(id: u32, priority: i64, m: serde_json::Value) -> Rule {
    Rule {
        id,
        category_id: id as u16,
        priority,
        enabled: true,
        origin: Origin::Builtin,
        name: format!("r{id}"),
        match_: RuleMatch::from_json(&m).unwrap(),
    }
}

fn with_app(f: FlowRecord, app: &str, cat: &str, confident: bool) -> FlowRecord {
    f.with_app(&AppInfo::new(app, cat, confident, None, false).unwrap())
}

fn base() -> FlowRecord {
    flow("192.168.1.10", "203.0.113.5", 6881, 6)
}

#[test]
fn app_matches_case_insensitively_and_needs_an_app() {
    let rs = RuleSet::compile(
        &[rule(1, 10, json!({"app": ["bittorrent"]}))],
        &HashMap::new(),
    );
    assert_eq!(
        rs.categorize(&with_app(base(), "BitTorrent", "", true)).1,
        Some(1)
    );
    assert_eq!(rs.categorize(&with_app(base(), "Steam", "", true)).1, None);
    assert_eq!(rs.categorize(&base()).1, None);
}

#[test]
fn app_category_matches_classifier_category() {
    let rs = RuleSet::compile(
        &[rule(2, 10, json!({"app_category": ["VPN"]}))],
        &HashMap::new(),
    );
    assert_eq!(
        rs.categorize(&with_app(base(), "WireGuard", "vpn", false))
            .1,
        Some(2)
    );
    assert_eq!(
        rs.categorize(&with_app(base(), "WireGuard", "", false)).1,
        None
    );
}

#[test]
fn app_combines_with_other_fields() {
    let rs = RuleSet::compile(
        &[rule(3, 10, json!({"app": ["Steam"], "proto": [17]}))],
        &HashMap::new(),
    );
    assert_eq!(
        rs.categorize(&with_app(base(), "Steam", "Game", true)).1,
        None
    );
    let udp = flow("192.168.1.10", "203.0.113.5", 27015, 17);
    assert_eq!(
        rs.categorize(&with_app(udp, "Steam", "Game", true)).1,
        Some(3)
    );
}

#[test]
fn validation_errors_name_the_field() {
    let long = "a".repeat(49);
    let many: Vec<String> = (0..65).map(|i| format!("app{i}")).collect();
    let cases = [
        (json!({"app": []}), "app"),
        (json!({"app": many}), "app"),
        (json!({"app": [long]}), "app[0]"),
        (json!({"app": ["ok", "bad\u{7}"]}), "app[1]"),
        (json!({"app_category": [""]}), "app_category[0]"),
        (json!({"app": "BitTorrent"}), "app"),
    ];
    for (m, field) in cases {
        let err = RuleMatch::from_json(&m).unwrap_err();
        let text = format!("{err:?}");
        assert!(text.contains(field), "{m} → {text}");
    }
    let ok: Vec<String> = (0..64).map(|i| format!("app{i}")).collect();
    assert!(RuleMatch::from_json(&json!({"app": ok})).is_ok());
}

#[test]
fn round_trips_through_json() {
    let m = RuleMatch::from_json(&json!({"app": ["Zoom"], "app_category": ["VoIP"]})).unwrap();
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v, json!({"app": ["Zoom"], "app_category": ["VoIP"]}));
    assert_eq!(RuleMatch::from_json(&v).unwrap(), m);
}

#[test]
fn confidence_depends_on_the_flow() {
    let rules = [
        rule(1, 40, json!({"hostname": ["*.example.com"]})),
        rule(2, 30, json!({"app": ["WireGuard"]})),
        rule(3, 20, json!({"asn": [64500]})),
        rule(4, 10, json!({"remote_port": [6881]})),
    ];
    let rs = RuleSet::compile(&rules, &HashMap::new());
    let conf = |f: &FlowRecord| rs.find(f).map(|r| r.confidence_for(f));

    let named = base().with_remote_host(Some("a.example.com".into()));
    assert_eq!(conf(&named), Some(Confidence::Detected));
    assert_eq!(
        conf(&with_app(base(), "WireGuard", "VPN", true)),
        Some(Confidence::Detected)
    );
    assert_eq!(
        conf(&with_app(base(), "WireGuard", "VPN", false)),
        Some(Confidence::Inferred)
    );
    let asn = base().with_geo(None, Some(64500), None);
    assert_eq!(conf(&asn), Some(Confidence::Inferred));
    assert_eq!(conf(&base()), Some(Confidence::Guessed));
}

#[test]
fn rule_level_confidence_of_app_rules_is_detected() {
    let m = RuleMatch::from_json(&json!({"app": ["Steam"]})).unwrap();
    assert_eq!(m.confidence(), Confidence::Detected);
}

#[test]
fn hostname_band_beats_app_band() {
    // Higher priority wins: hostname rules (900k) > app rules (850k–869,999).
    let rs = RuleSet::compile(
        &[
            rule(1, 900_000, json!({"hostname": ["*.netflix.com"]})),
            rule(2, 860_000, json!({"app": ["Netflix"]})),
        ],
        &HashMap::new(),
    );
    let f =
        with_app(base(), "Netflix", "Video", true).with_remote_host(Some("www.netflix.com".into()));
    assert_eq!(rs.categorize(&f).1, Some(1));
    assert_eq!(
        rs.categorize(&with_app(base(), "Netflix", "Video", true)).1,
        Some(2)
    );
}
