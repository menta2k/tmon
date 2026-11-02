//! Duplicate captures, failure suppression and hostile input (T063).

mod common;

use common::*;
use tmon_capture::fixtures::hello::{Hello, client_hello, record};
use tmon_capture::fixtures::scenarios::{self, EXPECTED};
use tmon_capture::fixtures::{ACK, addr, tcp, udp};

#[test]
fn replaying_a_scenario_twice_counts_each_connection_once() {
    let mut r = Rig::new();
    for name in ["tls_split_pq", "quic_v1_reordered", "http_host"] {
        let d = scenarios::datagrams(name);
        r.feed(&d);
        r.feed(&d);
    }
    let c = r.counters();
    assert_eq!((c.tls_started, c.tls_named), (1, 1), "{c:?}");
    assert_eq!((c.quic_started, c.quic_named), (1, 1), "{c:?}");
    assert_eq!(c.http_named, 1, "{c:?}");
    assert_eq!(c.tls_parse_error + c.quic_decrypt_error, 0);
}

#[test]
fn duplicate_segments_inside_one_hello_are_harmless() {
    let mut r = Rig::new();
    let d = scenarios::datagrams("tls_split_pq");
    // Every segment delivered twice, interleaved.
    let doubled: Vec<Vec<u8>> = d.iter().flat_map(|x| [x.clone(), x.clone()]).collect();
    r.feed(&doubled);
    let e = EXPECTED
        .iter()
        .find(|e| e.scenario == "tls_split_pq")
        .unwrap();
    assert_eq!(
        &*r.name(addr(e.client), addr(e.server), 6).unwrap().host,
        e.host
    );
    assert_eq!(r.counters().tls_started, 1);
}

#[test]
fn failed_connections_are_not_restarted() {
    let mut r = Rig::new();
    let (c, s) = (addr("192.168.88.30:41000"), addr("198.51.100.1:443"));
    // A TLS handshake record header followed by garbage → parse error.
    let mut bad = vec![0x16, 0x03, 0x01, 0x00, 0x30, 0x01, 0x00, 0x00, 0x2c];
    bad.extend(std::iter::repeat_n(0xff, 0x2c));
    r.feed(&[tcp(c, s, 100, ACK, &bad)]);
    assert_eq!(r.counters().tls_parse_error, 1);
    // The same bytes again, then a valid hello on the same 5-tuple: ignored.
    let good = record(&client_hello(&Hello {
        sni: Some("late.example"),
        alpn: &[],
        ech: false,
        pq_key_share: false,
    }));
    r.feed(&[
        tcp(c, s, 100, ACK, &bad),
        tcp(c, s, 100 + bad.len() as u32, ACK, &good),
    ]);
    let k = r.counters();
    assert_eq!(
        (k.tls_parse_error, k.tls_started, k.tls_named),
        (1, 1, 0),
        "{k:?}"
    );
    assert!(r.name(c, s, 6).is_none());
    assert_eq!(r.proc.reassembly_bytes(), 0);
}

#[test]
fn truncated_and_garbage_datagrams_are_counted_not_fatal() {
    let mut r = Rig::new();
    let valid = scenarios::datagrams("tls_split_pq").remove(0);
    for cut in 0..valid.len() {
        r.proc.handle(&valid[..cut], NOW);
    }
    let quic = scenarios::datagrams("quic_v1_rfc").remove(0);
    for cut in (0..quic.len()).step_by(7) {
        r.proc.handle(&quic[..cut], NOW);
    }
    // Bit flips across a QUIC Initial: decrypt errors, never a panic.
    for i in (60..quic.len()).step_by(13) {
        let mut q = quic.clone();
        q[i] ^= 0x5a;
        r.proc.handle(&q, NOW);
    }
    let c = r.counters();
    assert!(
        c.malformed_tzsp + c.malformed_frame + c.unsupported_l3 > 0,
        "{c:?}"
    );
    assert!(
        r.proc.reassembly_bytes() <= tmon_capture::Limits::default().max_reassembly_bytes_total
    );
}

#[test]
fn udp_to_port_53_from_a_lan_client_is_not_a_dns_answer() {
    let mut r = Rig::new();
    let q = tmon_capture::fixtures::dns::query("example.org");
    r.feed(&[udp(addr("192.168.88.10:5353"), addr("192.168.88.1:53"), &q)]);
    let c = r.counters();
    assert_eq!(
        (c.dns_responses, c.dns_bindings, c.dns_parse_error),
        (0, 0, 0),
        "{c:?}"
    );
}

fn hello(sni: &str) -> Vec<u8> {
    record(&client_hello(&Hello {
        sni: Some(sni),
        alpn: &[],
        ech: false,
        pq_key_share: false,
    }))
}

/// Review H1: a reused 4-tuple (new SYN, new ISN) is named again.
#[test]
fn a_new_connection_on_a_reused_tuple_is_named_again() {
    use tmon_capture::fixtures::tcp_stream;
    let mut r = Rig::new();
    let (c, s) = (addr("192.168.88.31:42000"), addr("198.51.100.2:443"));
    r.feed(&tcp_stream(c, s, 1_000, &hello("first.example"), 1400));
    assert_eq!(&*r.name(c, s, 6).unwrap().host, "first.example");
    // Retransmitted SYN of the same connection: no restart.
    r.feed(&[tcp(c, s, 1_000, tmon_capture::fixtures::SYN, &[])]);
    assert_eq!(r.counters().tcp_syn_seen, 1);
    r.feed(&tcp_stream(c, s, 900_000, &hello("second.example"), 1400));
    assert_eq!(&*r.name(c, s, 6).unwrap().host, "second.example");
    assert_eq!(r.counters().tls_named, 2);
}

/// Review H1: finished tuples are forgotten after `conn_ttl`.
#[test]
fn finished_tuples_expire_after_the_connection_ttl() {
    let mut r = Rig::new();
    let (c, s) = (addr("192.168.88.32:42001"), addr("198.51.100.3:443"));
    let h = hello("again.example");
    r.proc
        .handle(&tcp(c, s, 10, ACK, &[0x17, 0x03, 0x03, 0, 1, 0]), NOW); // mid-stream → done
    r.proc.handle(&tcp(c, s, 50, ACK, &h), NOW + 1_000);
    assert_eq!(r.counters().tls_started, 0, "still done");
    let later = NOW + tmon_capture::Limits::default().conn_ttl_ms + 1;
    r.proc.handle(&tcp(c, s, 50, ACK, &h), later);
    assert_eq!(r.counters().tls_named, 1);
}

/// Review H2: many overlapping out-of-order segments cannot exceed the per-connection cap.
#[test]
fn out_of_order_flood_stays_within_the_connection_cap() {
    let mut r = Rig::new();
    let cap = tmon_capture::Limits::default().max_reassembly_bytes_per_conn;
    let (c, s) = (addr("192.168.88.33:42002"), addr("198.51.100.4:443"));
    r.proc
        .handle(&tcp(c, s, 0, tmon_capture::fixtures::SYN, &[]), NOW);
    let mut first = hello("x.example");
    first.truncate(5); // a ClientHello record header so the connection is tracked as TLS
    r.proc.handle(&tcp(c, s, 1, ACK, &first[..1]), NOW);
    let seg = vec![0xab; 1_400];
    for off in 10..2_000u32 {
        r.proc.handle(&tcp(c, s, 1 + off, ACK, &seg), NOW);
        assert!(
            r.proc.reassembly_bytes() <= cap,
            "held {} > cap {cap}",
            r.proc.reassembly_bytes()
        );
    }
    assert_eq!(r.proc.pending_len(), 0, "the connection is given up");
}
