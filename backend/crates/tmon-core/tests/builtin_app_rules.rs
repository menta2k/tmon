//! Built-in application rules (feature 003, T033 / research R10).

mod common;

use common::flow;
use serde_json::json;
use std::collections::HashMap;
use tmon_core::rules::{Confidence, Origin, Rule, RuleMatch, RuleSet, load_builtin};
use tmon_core::{AppInfo, Categorizer, FlowRecord};

const APP_BAND: std::ops::RangeInclusive<i64> = 850_000..=869_999;

fn cat_id(name: &str) -> u16 {
    load_builtin()
        .unwrap()
        .categories
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("missing category {name}"))
        .id
}

fn builtin_set(extra: &[Rule]) -> RuleSet {
    let mut rules = load_builtin().unwrap().rules;
    rules.extend_from_slice(extra);
    RuleSet::compile(&rules, &HashMap::new())
}

fn app_flow(port: u16, proto: u8, app: &str, cat: &str, confident: bool) -> FlowRecord {
    flow("192.168.88.30", "198.51.100.20", port, proto)
        .with_app(&AppInfo::new(app, cat, confident, None, false).unwrap())
}

fn outcome(rs: &RuleSet, f: &FlowRecord) -> (u16, Confidence) {
    let r = rs.find(f).expect("a rule matches");
    (r.category_id, r.confidence_for(f))
}

#[test]
fn file_sharing_category_exists_with_id_15() {
    let b = load_builtin().unwrap();
    let c = b.categories.iter().find(|c| c.id == 15).expect("id 15");
    assert_eq!(c.name, "File Sharing");
    assert!(c.color.starts_with('#'));
}

#[test]
fn app_rules_live_in_their_bands() {
    let b = load_builtin().unwrap();
    let apps: Vec<_> = b
        .rules
        .iter()
        .filter(|r| r.match_.app.is_some() || r.match_.app_category.is_some())
        .collect();
    assert!(apps.len() >= 20, "about 25 app rules, got {}", apps.len());
    for r in apps {
        assert!(
            APP_BAND.contains(&r.priority),
            "{} at {}",
            r.name,
            r.priority
        );
        let named = r.match_.app.is_some();
        assert_eq!(
            named,
            r.priority >= 860_000,
            "{}: band matches kind",
            r.name
        );
    }
}

#[test]
fn named_apps_map_to_tmon_categories() {
    let rs = builtin_set(&[]);
    let cases = [
        (6881, 6, "BitTorrent", "Download", "File Sharing"),
        (51820, 17, "WireGuard", "VPN", "VPN/Tunnels"),
        (1194, 17, "OpenVPN", "VPN", "VPN/Tunnels"),
        (27015, 17, "Steam", "Game", "Gaming"),
        (27015, 17, "Source_Engine", "Game", "Gaming"),
        (443, 6, "Zoom", "Video", "Messaging/VoIP"),
        (3478, 17, "STUN", "Network", "Messaging/VoIP"),
        (3389, 6, "RDP", "RemoteAccess", "Remote Access"),
        (443, 6, "Netflix", "Video", "Streaming"),
    ];
    for (port, proto, app, ndpi, tmon) in cases {
        let f = app_flow(port, proto, app, ndpi, true);
        assert_eq!(
            outcome(&rs, &f),
            (cat_id(tmon), Confidence::Detected),
            "{app}"
        );
        let guessed = app_flow(port, proto, app, ndpi, false);
        assert_eq!(
            outcome(&rs, &guessed),
            (cat_id(tmon), Confidence::Inferred),
            "{app}"
        );
    }
}

#[test]
fn ndpi_category_fallback_applies_without_a_named_rule() {
    let rs = builtin_set(&[]);
    let f = app_flow(40000, 17, "SomeNewVPN", "VPN", true);
    let r = rs.find(&f).unwrap();
    assert_eq!(r.category_id, cat_id("VPN/Tunnels"));
    assert!((850_000..860_000).contains(&r.priority));
}

#[test]
fn hostname_rule_beats_app_rule() {
    let rs = builtin_set(&[]);
    let f = app_flow(443, 6, "BitTorrent", "Download", true)
        .with_remote_host(Some("www.netflix.com".into()));
    assert_eq!(rs.categorize(&f).0, cat_id("Streaming"));
}

#[test]
fn user_app_rule_beats_builtin() {
    let work = Rule {
        id: 10_001,
        category_id: 200,
        priority: 1_000_000,
        enabled: true,
        origin: Origin::User,
        name: "Zoom is work".into(),
        match_: RuleMatch::from_json(&json!({"app": ["Zoom"]})).unwrap(),
    };
    let rs = builtin_set(&[work]);
    let f = app_flow(443, 6, "Zoom", "Video", true);
    assert_eq!(rs.categorize(&f), (200, Some(10_001)));
}

#[test]
fn app_rule_beats_port_rule() {
    let rs = builtin_set(&[]);
    // BitTorrent on the HTTPS port: the app wins over the web catch-all.
    let f = app_flow(443, 6, "BitTorrent", "Download", true);
    assert_eq!(rs.categorize(&f).0, cat_id("File Sharing"));
}
