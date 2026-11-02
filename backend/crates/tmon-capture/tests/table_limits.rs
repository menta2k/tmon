mod common;

use std::net::IpAddr;
use std::sync::Arc;
use tmon_capture::Limits;
use tmon_capture::health::Health;
use tmon_capture::table::ConnTable;
use tmon_core::naming::{ConnKey, ConnName, ConnectionNamer, NameSource};

fn key(lan_port: u16) -> ConnKey {
    ConnKey {
        lan_ip: "192.168.88.10".parse().unwrap(),
        lan_port,
        remote_ip: "142.250.1.1".parse().unwrap(),
        remote_port: 443,
        proto: 6,
    }
}

fn name(h: &str, s: NameSource) -> Arc<ConnName> {
    Arc::new(ConnName::new(h, s).unwrap())
}

fn table(limits: Limits) -> (Arc<Health>, ConnTable) {
    let h = Arc::new(Health::default());
    (h.clone(), ConnTable::new(&limits, h))
}

const TTL: i64 = 600_000;

#[test]
fn insert_replaces_and_lookup_refreshes_ttl() {
    let (h, t) = table(Limits::default());
    t.insert(key(1), name("a.example", NameSource::Tls), 0);
    t.insert(key(1), name("b.example", NameSource::Quic), 10);
    assert_eq!(t.conns_len(), 1);
    let l = t.lookup(&key(1), TTL);
    assert_eq!(&*l.connection.unwrap().host, "b.example");
    // refreshed at TTL: still alive at 2*TTL, gone just after.
    assert!(t.lookup(&key(1), 2 * TTL).connection.is_some());
    assert!(t.lookup(&key(1), 3 * TTL + 1).connection.is_none());
    let c = h.counters.values();
    assert_eq!(c.conn_evicted_ttl, 1);
    assert_eq!((c.lookups, c.lookup_hits_conn), (3, 2));
    assert_eq!(c.conn_evicted_lru, 0, "replacing a key is not an eviction");
}

#[test]
fn exact_connection_and_device_binding_are_both_returned() {
    let (h, t) = table(Limits::default());
    let lan: IpAddr = "192.168.88.10".parse().unwrap();
    let remote: IpAddr = "142.250.1.1".parse().unwrap();
    t.bind(lan, remote, name("www.youtube.com", NameSource::Dns), 1_000);
    let l = t.lookup(&key(9), 0);
    assert!(l.connection.is_none());
    assert_eq!(&*l.device.unwrap().host, "www.youtube.com");
    t.insert(key(9), name("rr1.googlevideo.com", NameSource::Tls), 0);
    let l = t.lookup(&key(9), 0);
    assert_eq!(&*l.connection.unwrap().host, "rr1.googlevideo.com");
    assert!(l.device.is_some());
    assert!(t.lookup(&key(9), 1_001).device.is_none(), "binding expired");
    let c = h.counters.values();
    assert_eq!((c.lookup_hits_dns, c.lookup_hits_conn), (1, 2));
    assert_eq!(c.bindings_evicted, 1);
    // other devices don't see this device's binding
    let mut other = key(9);
    other.lan_ip = "192.168.88.11".parse().unwrap();
    assert!(t.lookup(&other, 0).device.is_none());
}

#[test]
fn lru_bounds_connections_and_bindings() {
    let limits = Limits {
        max_connections: 32,
        max_device_bindings: 16,
        ..Limits::default()
    };
    let (h, t) = table(limits);
    for p in 0..1_000u16 {
        t.insert(key(p), name("x.example", NameSource::Tls), 0);
        let remote = IpAddr::from([10, 0, (p >> 8) as u8, p as u8]);
        t.bind(
            "192.168.88.10".parse().unwrap(),
            remote,
            name("y.example", NameSource::Dns),
            10,
        );
    }
    assert!(t.conns_len() <= 32);
    assert!(t.bindings_len() <= 16);
    let c = h.counters.values();
    assert_eq!(c.conn_evicted_lru as usize, 1_000 - t.conns_len());
    assert_eq!(c.bindings_evicted as usize, 1_000 - t.bindings_len());
}

#[test]
fn sweep_drops_expired_entries() {
    let (_h, t) = table(Limits::default());
    t.insert(key(1), name("a.example", NameSource::Tls), 0);
    t.insert(key(2), name("b.example", NameSource::Tls), TTL);
    t.bind(
        "192.168.88.10".parse().unwrap(),
        "1.1.1.1".parse().unwrap(),
        name("c.example", NameSource::Dns),
        5,
    );
    assert_eq!(t.sweep(TTL + 1), 2);
    assert_eq!((t.conns_len(), t.bindings_len()), (1, 0));
}

/// 1M random keys: the table never exceeds its bound (run with --ignored).
#[test]
#[ignore]
fn flood() {
    let (_h, t) = table(Limits::default());
    let n = name("x.example", NameSource::Tls);
    for i in 0..1_000_000u32 {
        let mut k = key(i as u16);
        k.remote_ip = IpAddr::from(i.to_be_bytes());
        t.insert(k, Arc::clone(&n), 0);
    }
    assert!(t.conns_len() <= Limits::default().max_connections);
}

/// T060 / FR-016: once named (or given up), no payload bytes are retained.
#[test]
fn named_connections_keep_no_payload() {
    use tmon_capture::fixtures::scenarios::{self, EXPECTED};
    let mut r = common::Rig::new();
    for name in [
        "tls_split_pq",
        "tls_ech",
        "quic_v1_rfc",
        "quic_v1_reordered",
        "quic_v2_rfc",
        "http_host",
        "shared_cdn",
    ] {
        r.feed(&scenarios::datagrams(name));
        assert_eq!(r.proc.reassembly_bytes(), 0, "{name} left bytes buffered");
        assert_eq!(r.proc.pending_len(), 0, "{name} left a pending connection");
    }
    for e in EXPECTED.iter().filter(|e| e.source != "dns") {
        assert!(
            r.name(addr(e.client), addr(e.server), e.proto).is_some(),
            "{e:?}"
        );
    }
}

#[test]
fn abandoned_hellos_are_freed_by_the_timeout() {
    use tmon_capture::fixtures::scenarios;
    let mut r = common::Rig::new();
    r.feed(&scenarios::datagrams("wsl_truncated"));
    assert!(r.proc.reassembly_bytes() > 0, "first segments are buffered");
    let timeout = Limits::default().reassembly_timeout_ms;
    r.proc.sweep(common::NOW + timeout + 1_000);
    assert_eq!(r.proc.reassembly_bytes(), 0);
    assert_eq!(r.proc.pending_len(), 0);
    assert_eq!(
        r.counters().tls_incomplete,
        u64::from(scenarios::WSL_CONNECTIONS)
    );
}

fn addr(s: &str) -> std::net::SocketAddr {
    s.parse().unwrap()
}
