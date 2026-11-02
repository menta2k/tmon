//! Application step of the enrichment pipeline (feature 003, data-model.md §2).

use serde_json::json;
use std::sync::{Arc, Mutex};
use tmon_core::app::{AppInfo, ConnectionClassifier};
use tmon_core::naming::{ConnKey, ConnName, ConnectionNamer, NameLookup, NameSource};
use tmon_core::rules::{Origin, Rule, RuleMatch, RuleSet};
use tmon_core::{LocalNetworks, RawFlow};
use tmon_enrich::pipeline::{Pipeline, PipelineDeps};

struct Classifier {
    info: Option<Arc<AppInfo>>,
    seen: Mutex<Vec<ConnKey>>,
}

impl ConnectionClassifier for Classifier {
    fn classify(&self, key: &ConnKey, _now: i64) -> Option<Arc<AppInfo>> {
        self.seen.lock().unwrap().push(*key);
        self.info.clone()
    }
}

struct Namer(Arc<ConnName>);

impl ConnectionNamer for Namer {
    fn lookup(&self, _key: &ConnKey, _now: i64) -> NameLookup {
        NameLookup {
            connection: Some(Arc::clone(&self.0)),
            device: None,
        }
    }
}

fn raw() -> RawFlow {
    RawFlow {
        ts_ms: 1_767_225_600_000,
        ts_start_ms: 1_767_225_600_000,
        exporter_id: 1,
        in_if: 1,
        out_if: 2,
        src_ip: "192.168.88.10".parse().unwrap(),
        dst_ip: "203.0.113.7".parse().unwrap(),
        src_port: 51000,
        dst_port: 51820,
        proto: 17,
        tcp_flags: 0,
        bytes: 1000,
        packets: 1,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    }
}

fn classifier(app: &str, cat: &str, confident: bool, host: Option<&str>) -> Arc<Classifier> {
    Arc::new(Classifier {
        info: Some(Arc::new(
            AppInfo::new(app, cat, confident, host, true).unwrap(),
        )),
        seen: Mutex::new(Vec::new()),
    })
}

fn pipeline() -> Pipeline {
    Pipeline::new(PipelineDeps::minimal(), LocalNetworks::defaults())
}

#[test]
fn sets_app_fields_and_uses_the_lan_side_key() {
    let p = pipeline();
    let c = classifier("WireGuard", "VPN", true, None);
    assert!(p.set_classifier(c.clone()));
    assert!(!p.set_classifier(c.clone()), "set once");
    let f = p.enrich(&raw());
    assert_eq!(f.app.as_deref(), Some("WireGuard"));
    assert!(f.app_confident);
    assert_eq!(f.app_category.as_deref(), Some("VPN"));
    assert_eq!(f.remote_host, None);
    let key = c.seen.lock().unwrap()[0];
    assert_eq!(key.lan_ip.to_string(), "192.168.88.10");
    assert_eq!(key.remote_port, 51820);
}

#[test]
fn classifier_host_fills_only_an_unnamed_flow() {
    let p = pipeline();
    assert!(p.set_classifier(classifier("Zoom", "Video", true, Some("zoom.us"))));
    let f = p.enrich(&raw());
    assert_eq!(f.remote_host.as_deref(), Some("zoom.us"));
    assert_eq!(f.remote_host_source, Some(NameSource::Dpi));

    let named = pipeline();
    let n = ConnName::new("www.netflix.com", NameSource::Tls).unwrap();
    assert!(named.set_namer(Arc::new(Namer(Arc::new(n)))));
    assert!(named.set_classifier(classifier("Netflix", "Video", true, Some("other.example"))));
    let f = named.enrich(&raw());
    assert_eq!(f.remote_host.as_deref(), Some("www.netflix.com"));
    assert_eq!(f.remote_host_source, Some(NameSource::Tls));
    assert_eq!(f.app.as_deref(), Some("Netflix"));
}

#[test]
fn categorization_sees_the_app_category() {
    let p = pipeline();
    let rule = Rule {
        id: 1,
        category_id: 8,
        priority: 855_000,
        enabled: true,
        origin: Origin::Builtin,
        name: "VPN apps".into(),
        match_: RuleMatch::from_json(&json!({"app_category": ["vpn"]})).unwrap(),
    };
    p.set_rules(RuleSet::compile(&[rule], &Default::default()));
    assert!(p.set_classifier(classifier("OpenVPN", "VPN", false, None)));
    let f = p.enrich(&raw());
    assert_eq!((f.category_id, f.rule_id), (8, Some(1)));
}

#[test]
fn no_classifier_or_no_answer_leaves_the_record_unchanged() {
    let plain = pipeline().enrich(&raw());
    let p = pipeline();
    assert!(p.set_classifier(Arc::new(Classifier {
        info: None,
        seen: Mutex::new(Vec::new()),
    })));
    assert_eq!(p.enrich(&raw()), plain);
    assert_eq!(plain.app, None);
    assert!(!plain.app_confident);
}
