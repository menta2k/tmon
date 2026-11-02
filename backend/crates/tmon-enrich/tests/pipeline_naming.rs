use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tmon_core::naming::{ConnKey, ConnName, ConnectionNamer, NameLookup, NameSource};
use tmon_core::rules::{Origin, Rule, RuleMatch, RuleSet};
use tmon_core::{LocalNetworks, RawFlow};
use tmon_enrich::pipeline::{Pipeline, PipelineDeps};

#[derive(Default)]
struct Stub {
    connection: Option<Arc<ConnName>>,
    device: Option<Arc<ConnName>>,
    seen: Mutex<Vec<ConnKey>>,
}

impl ConnectionNamer for Stub {
    fn lookup(&self, key: &ConnKey, _now: i64) -> NameLookup {
        self.seen.lock().unwrap().push(*key);
        NameLookup {
            connection: self.connection.clone(),
            device: self.device.clone(),
        }
    }
}

fn name(h: &str, s: NameSource) -> Option<Arc<ConnName>> {
    Some(Arc::new(ConnName::new(h, s).unwrap()))
}

fn raw() -> RawFlow {
    RawFlow {
        ts_ms: 1_767_225_600_000,
        ts_start_ms: 1_767_225_600_000,
        exporter_id: 1,
        in_if: 1,
        out_if: 2,
        src_ip: "192.168.88.10".parse().unwrap(),
        dst_ip: "142.250.1.1".parse().unwrap(),
        src_port: 51000,
        dst_port: 443,
        proto: 6,
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

fn pipeline(stub: Option<Arc<Stub>>, router: Option<&str>) -> Pipeline {
    let p = Pipeline::new(PipelineDeps::minimal(), LocalNetworks::defaults());
    if let Some(s) = stub {
        assert!(p.set_namer(s));
    }
    if let Some(r) = router {
        let mut m = HashMap::new();
        m.insert("142.250.1.1".parse().unwrap(), Arc::<str>::from(r));
        p.set_remote_names(m);
    }
    p
}

fn host_and_source(p: &Pipeline) -> (Option<String>, Option<NameSource>) {
    let f = p.enrich(&raw());
    (f.remote_host.map(|h| h.to_string()), f.remote_host_source)
}

#[test]
fn exact_connection_wins_and_key_is_lan_side() {
    let stub = Arc::new(Stub {
        connection: name("rr1.googlevideo.com", NameSource::Quic),
        device: name("www.youtube.com", NameSource::Dns),
        ..Stub::default()
    });
    let p = pipeline(Some(stub.clone()), Some("router.example"));
    assert_eq!(
        host_and_source(&p),
        (Some("rr1.googlevideo.com".into()), Some(NameSource::Quic))
    );
    let k = stub.seen.lock().unwrap()[0];
    assert_eq!(
        (k.lan_ip.to_string(), k.lan_port, k.remote_port, k.proto),
        ("192.168.88.10".into(), 51000, 443, 6)
    );
}

#[test]
fn device_binding_then_router_cache() {
    let dns = Arc::new(Stub {
        device: name("www.youtube.com", NameSource::Dns),
        ..Stub::default()
    });
    assert_eq!(
        host_and_source(&pipeline(Some(dns), Some("r.example"))),
        (Some("www.youtube.com".into()), Some(NameSource::Dns))
    );
    let none = Arc::new(Stub::default());
    assert_eq!(
        host_and_source(&pipeline(Some(none), Some("r.example"))),
        (Some("r.example".into()), Some(NameSource::Router))
    );
}

#[test]
fn ech_outer_name_is_the_last_resort() {
    let ech = Arc::new(
        ConnName::new("cloudflare-ech.com", NameSource::Tls)
            .unwrap()
            .with_ech_outer(true),
    );
    let s = || {
        Arc::new(Stub {
            connection: Some(ech.clone()),
            ..Stub::default()
        })
    };
    assert_eq!(
        host_and_source(&pipeline(Some(s()), Some("real.example")))
            .0
            .as_deref(),
        Some("real.example")
    );
    assert_eq!(
        host_and_source(&pipeline(Some(s()), None)),
        (Some("cloudflare-ech.com".into()), Some(NameSource::Tls))
    );
}

#[test]
fn aliases_reach_the_rules() {
    let n = ConnName::new("media.unknown.example", NameSource::Dns)
        .unwrap()
        .with_aliases(&["rr9.googlevideo.com"]);
    let stub = Arc::new(Stub {
        device: Some(Arc::new(n)),
        ..Stub::default()
    });
    let p = pipeline(Some(stub), None);
    let rule = Rule {
        id: 77,
        category_id: 2,
        priority: 1,
        enabled: true,
        origin: Origin::User,
        name: "yt".into(),
        match_: RuleMatch {
            hostname: Some(vec!["*.googlevideo.com".into()]),
            ..RuleMatch::default()
        },
    };
    p.set_rules(RuleSet::compile(&[rule], &HashMap::new()));
    let f = p.enrich(&raw());
    assert_eq!((f.category_id, f.rule_id), (2, Some(77)));
    assert_eq!(f.remote_aliases.as_deref().map(|a| a.len()), Some(1));
}

#[test]
fn without_a_namer_behaviour_is_unchanged() {
    assert_eq!(host_and_source(&pipeline(None, None)), (None, None));
    assert_eq!(
        host_and_source(&pipeline(None, Some("r.example"))),
        (Some("r.example".into()), Some(NameSource::Router))
    );
    let p = pipeline(None, None);
    assert!(p.set_namer(Arc::new(Stub::default())));
    assert!(!p.set_namer(Arc::new(Stub::default())), "set once");
}
