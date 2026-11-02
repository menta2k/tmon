mod common;

use common::flow;
use serde_json::json;
use std::collections::HashMap;
use std::net::IpAddr;
use tmon_core::rules::{Origin, Rule, RuleMatch, RuleSet, USER_PRIORITY_BASE};
use tmon_core::{Categorizer, Direction, OTHER_ID};

fn rule(id: u32, cat: u16, prio: i64, m: serde_json::Value) -> Rule {
    let origin = if prio >= USER_PRIORITY_BASE {
        Origin::User
    } else {
        Origin::Builtin
    };
    Rule {
        id,
        category_id: cat,
        priority: prio,
        enabled: true,
        origin,
        name: format!("r{id}"),
        match_: RuleMatch::from_json(&m).expect("valid match"),
    }
}

fn set(rules: &[Rule]) -> RuleSet {
    RuleSet::compile(rules, &HashMap::new())
}

#[test]
fn each_criterion_matches() {
    let base = flow("192.168.1.10", "203.0.113.5", 443, 6)
        .with_geo(Some("DE".into()), Some(64500), None)
        .with_remote_host(Some("Video.Example.com".into()));
    let cases = vec![
        json!({"proto": [6]}),
        json!({"remote_port": [443]}),
        json!({"remote_port": ["400-500"]}),
        json!({"local_port": ["50000-50001"]}),
        json!({"remote_cidr": ["203.0.113.0/24"]}),
        json!({"local_host": ["192.168.1.10"]}),
        json!({"local_host": ["192.168.1.0/24"]}),
        json!({"asn": [64500]}),
        json!({"country": ["de"]}),
        json!({"interface": [2]}),
        json!({"hostname": ["*.example.com"]}),
        json!({"direction": ["up"]}),
    ];
    for m in cases {
        let rs = set(&[rule(1, 5, 10, m.clone())]);
        assert_eq!(
            rs.categorize(&base),
            (5, Some(1)),
            "criterion {m} should match"
        );
    }
}

#[test]
fn each_criterion_rejects() {
    let base = flow("192.168.1.10", "203.0.113.5", 443, 6);
    let cases = vec![
        json!({"proto": [17]}),
        json!({"remote_port": [80]}),
        json!({"remote_port": ["27015-27030"]}),
        json!({"remote_cidr": ["2001:db8::/32"]}),
        json!({"local_host": ["10.0.0.1"]}),
        json!({"asn": [1]}),
        json!({"country": ["US"]}),
        json!({"interface": [9]}),
        json!({"hostname": ["*.example.com"]}),
        json!({"direction": ["down"]}),
    ];
    for m in cases {
        let rs = set(&[rule(1, 5, 10, m.clone())]);
        assert_eq!(
            rs.categorize(&base),
            (OTHER_ID, None),
            "criterion {m} should not match"
        );
    }
}

#[test]
fn and_across_fields_or_within_list() {
    let rs = set(&[rule(
        1,
        3,
        10,
        json!({"proto": [17], "remote_port": ["27015-27030", 3074]}),
    )]);
    assert_eq!(rs.categorize(&flow("10.0.0.2", "1.2.3.4", 27020, 17)).0, 3);
    assert_eq!(rs.categorize(&flow("10.0.0.2", "1.2.3.4", 3074, 17)).0, 3);
    assert_eq!(
        rs.categorize(&flow("10.0.0.2", "1.2.3.4", 27020, 6)).0,
        OTHER_ID
    );
}

#[test]
fn higher_priority_wins_and_user_beats_builtin() {
    let rules = vec![
        rule(1, 1, 100_000, json!({"remote_port": [443]})),
        rule(2, 2, 900_000, json!({"remote_cidr": ["1.2.3.0/24"]})),
        rule(10_001, 7, USER_PRIORITY_BASE, json!({"remote_port": [443]})),
    ];
    let rs = set(&rules);
    assert_eq!(
        rs.categorize(&flow("10.0.0.2", "1.2.3.4", 443, 6)),
        (7, Some(10_001))
    );
    let without_user = set(&rules[..2]);
    assert_eq!(
        without_user.categorize(&flow("10.0.0.2", "1.2.3.4", 443, 6)),
        (2, Some(2))
    );
    assert_eq!(
        without_user.categorize(&flow("10.0.0.2", "9.9.9.9", 443, 6)),
        (1, Some(1))
    );
}

#[test]
fn disabled_rules_are_skipped() {
    let mut r = rule(1, 4, 10, json!({"proto": [6]}));
    r.enabled = false;
    assert_eq!(
        set(&[r]).categorize(&flow("10.0.0.2", "1.1.1.1", 1, 6)),
        (OTHER_ID, None)
    );
}

#[test]
fn ipv6_cidr_and_mac_resolution() {
    let rs = set(&[rule(1, 6, 10, json!({"remote_cidr": ["2001:db8::/32"]}))]);
    assert_eq!(rs.categorize(&flow("fd00::1", "2001:db8::42", 443, 6)).0, 6);

    let mac_rule = rule(2, 8, 10, json!({"local_host": ["AA-BB-CC-DD-EE-FF"]}));
    let mut macs: HashMap<String, Vec<IpAddr>> = HashMap::new();
    macs.insert(
        "aa:bb:cc:dd:ee:ff".into(),
        vec!["192.168.1.50".parse().unwrap()],
    );
    let rs = RuleSet::compile(std::slice::from_ref(&mac_rule), &macs);
    assert_eq!(rs.categorize(&flow("192.168.1.50", "1.1.1.1", 1, 6)).0, 8);
    assert_eq!(
        rs.categorize(&flow("192.168.1.51", "1.1.1.1", 1, 6)).0,
        OTHER_ID
    );
    let unresolved = set(&[mac_rule]);
    assert_eq!(
        unresolved
            .categorize(&flow("192.168.1.50", "1.1.1.1", 1, 6))
            .0,
        OTHER_ID
    );
}

#[test]
fn hostname_rules_need_a_hostname() {
    let rs = set(&[rule(1, 2, 10, json!({"hostname": ["*.googlevideo.com"]}))]);
    let f = flow("10.0.0.2", "1.1.1.1", 443, 17);
    assert_eq!(rs.categorize(&f).0, OTHER_ID);
    let named = f.with_remote_host(Some("rr3---sn-x.GOOGLEVIDEO.com".into()));
    assert_eq!(rs.categorize(&named).0, 2);
}

#[test]
fn validation_reports_indexed_fields() {
    let err = RuleMatch::from_json(&json!({
        "remote_cidr": ["10.0.0.0/8", "bogus"],
        "remote_port": ["30-20"],
        "country": ["USA"],
        "direction": ["sideways"],
        "nope": [1]
    }))
    .unwrap_err();
    let f = err.fields();
    assert_eq!(
        f.get("remote_cidr[1]").map(String::as_str),
        Some("invalid CIDR")
    );
    assert!(f.contains_key("remote_port[0]"));
    assert!(f.contains_key("country[0]"));
    assert!(f.contains_key("direction[0]"));
    assert!(f.contains_key("nope"));
    assert!(!f.contains_key("remote_cidr[0]"));
}

#[test]
fn rule_validation() {
    let mut r = rule(1, 1, 10, json!({"proto": [6]}));
    assert!(r.validate().is_ok());
    r.name = String::new();
    r.origin = Origin::User;
    let e = r.validate().unwrap_err();
    assert!(e.fields().contains_key("name"));
    assert!(e.fields().contains_key("priority"));
    let empty = RuleMatch::from_json(&json!({})).unwrap();
    assert!(empty.is_empty());
    let mut r2 = rule(1, 1, 10, json!({"proto": [6]}));
    r2.match_ = empty;
    assert!(r2.validate().unwrap_err().fields().contains_key("match"));
}

#[test]
fn match_round_trips_through_json() {
    let m = RuleMatch::from_json(&json!({"remote_port": [443, "1000-2000"], "local_host": ["aa:bb:cc:dd:ee:ff", "10.0.0.1", "10.0.0.0/8"]})).unwrap();
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(
        v,
        json!({"remote_port": [443, "1000-2000"], "local_host": ["aa:bb:cc:dd:ee:ff", "10.0.0.1", "10.0.0.0/8"]})
    );
    let back: RuleMatch = serde_json::from_value(v).unwrap();
    assert_eq!(back, m);
    let _ = Direction::Up;
}
