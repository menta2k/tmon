mod common;

use common::*;
use hickory_proto::op::ResponseCode;
use std::net::IpAddr;
use tmon_capture::dns::{self, clamp_ttl};
use tmon_capture::fixtures::dns::{Answer, query, response};
use tmon_capture::fixtures::{addr, udp};
use tmon_core::naming::{ConnKey, ConnectionNamer, NameSource};

const RESOLVER: &str = "192.168.88.1:53";

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn youtube() -> Vec<u8> {
    response(
        "www.youtube.com",
        &[
            Answer::Cname("www.youtube.com", "youtube-ui.l.google.com", 300),
            Answer::Cname("youtube-ui.l.google.com", "edge.l.google.com", 300),
            Answer::Addr("edge.l.google.com", ip("142.250.1.1"), 120),
            Answer::Addr("edge.l.google.com", ip("142.250.1.2"), 7200),
            Answer::Addr("unrelated.example", ip("10.9.9.9"), 300),
        ],
        ResponseCode::NoError,
    )
}

#[test]
fn cname_chain_yields_bindings_for_each_address() {
    let b = dns::bindings(&youtube()).unwrap();
    assert_eq!(
        b.len(),
        2,
        "unrelated owner is not reachable from the query"
    );
    assert_eq!(b[0].addr, ip("142.250.1.1"));
    assert_eq!(b[0].qname, "www.youtube.com");
    assert_eq!(
        b[0].aliases,
        ["youtube-ui.l.google.com", "edge.l.google.com"]
    );
    assert_eq!((b[0].ttl_secs, b[1].ttl_secs), (600, 3600), "clamped");
    assert_eq!(
        (clamp_ttl(0), clamp_ttl(1800), clamp_ttl(u32::MAX)),
        (600, 1800, 3600)
    );
}

#[test]
fn negative_answers_queries_and_garbage() {
    let nx = response("nope.example", &[], ResponseCode::NXDomain);
    assert!(dns::bindings(&nx).unwrap().is_empty());
    assert!(dns::bindings(&query("www.example.com")).unwrap().is_empty());
    assert!(dns::bindings(&[1, 2, 3]).is_err());
    let looped = response(
        "a.example",
        &[
            Answer::Cname("a.example", "b.example", 60),
            Answer::Cname("b.example", "a.example", 60),
        ],
        ResponseCode::NoError,
    );
    assert!(dns::bindings(&looped).unwrap().is_empty());
}

#[test]
fn answers_bind_per_device_and_feed_remote_names() {
    let mut r = Rig::new();
    let dev_a = addr("192.168.88.10:5353");
    let dev_b = addr("192.168.88.11:5353");
    r.feed(&[udp(addr(RESOLVER), dev_a, &youtube())]);
    let other = response(
        "other.example",
        &[Answer::Addr("other.example", ip("142.250.1.1"), 600)],
        ResponseCode::NoError,
    );
    r.feed(&[udp(addr(RESOLVER), dev_b, &other)]);
    let key = |lan: &str| ConnKey {
        lan_ip: ip(lan),
        lan_port: 40000,
        remote_ip: ip("142.250.1.1"),
        remote_port: 8080,
        proto: 6,
    };
    let a = r
        .out
        .table
        .lookup(&key("192.168.88.10"), NOW)
        .device
        .unwrap();
    assert_eq!((&*a.host, a.source), ("www.youtube.com", NameSource::Dns));
    assert_eq!(a.aliases.len(), 2);
    let b = r
        .out
        .table
        .lookup(&key("192.168.88.11"), NOW)
        .device
        .unwrap();
    assert_eq!(&*b.host, "other.example");
    // expires after the clamped TTL (600 s)
    assert!(
        r.out
            .table
            .lookup(&key("192.168.88.10"), NOW + 600_001)
            .device
            .is_none()
    );

    let c = r.counters();
    assert_eq!(
        (c.dns_responses, c.dns_bindings, c.dns_parse_error),
        (2, 3, 0)
    );
    let drained = r.out.names.drain();
    assert_eq!(drained.len(), 2, "one entry per remote IP (latest wins)");
    assert!(r.out.names.is_empty());

    // newer answer replaces the older binding
    let newer = response(
        "music.youtube.com",
        &[Answer::Addr("music.youtube.com", ip("142.250.1.1"), 600)],
        ResponseCode::NoError,
    );
    r.feed(&[udp(addr(RESOLVER), dev_a, &newer)]);
    assert_eq!(
        &*r.out
            .table
            .lookup(&key("192.168.88.10"), NOW)
            .device
            .unwrap()
            .host,
        "music.youtube.com"
    );

    r.feed(&[udp(addr(RESOLVER), dev_a, &[0xde, 0xad])]);
    assert_eq!(r.counters().dns_parse_error, 1);
}
