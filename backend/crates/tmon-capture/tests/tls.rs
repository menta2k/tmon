mod common;

use common::*;
use tmon_capture::fixtures::hello::{Hello, client_hello, record};
use tmon_capture::fixtures::{self, ACK, addr, tcp_stream};
use tmon_capture::{Limits, tls};
use tmon_core::naming::NameSource;

const CLIENT: &str = "192.168.88.10:51000";
const SERVER: &str = "142.250.1.1:443";

fn hello(sni: &str, pq: bool, ech: bool) -> Vec<u8> {
    record(&client_hello(&Hello {
        sni: Some(sni),
        alpn: &["h2", "http/1.1"],
        ech,
        pq_key_share: pq,
    }))
}

#[test]
fn single_record_hello_is_named_with_alpn() {
    let mut r = Rig::new();
    r.feed(&tcp_stream(
        addr(CLIENT),
        addr(SERVER),
        1000,
        &hello("WWW.Example.COM", false, false),
        1460,
    ));
    let n = r.name(addr(CLIENT), addr(SERVER), 6).expect("named");
    assert_eq!(
        (&*n.host, n.source, n.alpn.as_deref(), n.ech_outer),
        ("www.example.com", NameSource::Tls, Some("h2"), false)
    );
    let c = r.counters();
    assert_eq!(
        (
            c.tls_started,
            c.tls_named,
            c.tcp_syn_seen,
            c.tcp_payload_first_seen
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(r.proc.reassembly_bytes(), 0, "no payload kept after naming");
}

#[test]
fn post_quantum_hello_split_over_segments_in_any_order() {
    let rec = hello("rr3.googlevideo.com", true, false);
    assert!(
        rec.len() > 1460,
        "PQ hello spans two segments ({} bytes)",
        rec.len()
    );
    let segs = tcp_stream(addr(CLIENT), addr(SERVER), u32::MAX - 100, &rec, 1000); // seq wraps
    for order in [vec![0, 1, 2], vec![0, 2, 1], vec![0, 2, 2, 1, 1]] {
        let mut r = Rig::new();
        let feed: Vec<Vec<u8>> = order.iter().map(|i| segs[*i].clone()).collect();
        r.feed(&feed);
        let n = r
            .name(addr(CLIENT), addr(SERVER), 6)
            .unwrap_or_else(|| panic!("{order:?}"));
        assert_eq!(&*n.host, "rr3.googlevideo.com");
        assert_eq!(r.counters().tls_started, 1);
    }
}

#[test]
fn hello_without_syn_seen_starts_from_the_record_header() {
    let mut r = Rig::new();
    let segs = tcp_stream(
        addr(CLIENT),
        addr(SERVER),
        5,
        &hello("a.example", true, false),
        1000,
    );
    r.feed(&segs[1..]);
    assert!(r.name(addr(CLIENT), addr(SERVER), 6).is_some());
    // A mid-stream segment without a start is ignored without counting.
    let mut r = Rig::new();
    r.feed(&segs[2..]);
    assert!(r.name(addr(CLIENT), addr(SERVER), 6).is_none());
    assert_eq!(r.counters().tls_started, 0);
}

#[test]
fn oversized_or_never_completed_hello_is_incomplete() {
    let limits = Limits {
        max_reassembly_bytes_per_conn: 1024,
        ..Limits::default()
    };
    let mut r = Rig::with_limits(limits);
    r.feed(&tcp_stream(
        addr(CLIENT),
        addr(SERVER),
        1,
        &hello("big.example", true, false),
        800,
    ));
    assert!(r.name(addr(CLIENT), addr(SERVER), 6).is_none());
    assert_eq!(r.counters().tls_incomplete, 1);

    // First segment only, then the 10 s timeout passes.
    let mut r = Rig::new();
    let segs = tcp_stream(
        addr(CLIENT),
        addr(SERVER),
        1,
        &hello("slow.example", true, false),
        1000,
    );
    r.feed(&segs[..2]);
    assert!(r.proc.reassembly_bytes() > 0);
    r.proc.handle(&segs[0], NOW + 10_001);
    assert_eq!(r.counters().tls_incomplete, 1);
    assert_eq!(r.proc.reassembly_bytes(), 0);
    assert_eq!(r.out.health.window(NOW + 10_001).tls_incomplete, 1);
}

#[test]
fn ech_marks_the_outer_name() {
    let mut r = Rig::new();
    r.feed(&tcp_stream(
        addr(CLIENT),
        addr(SERVER),
        1,
        &hello("cloudflare-ech.com", false, true),
        1460,
    ));
    let n = r.name(addr(CLIENT), addr(SERVER), 6).unwrap();
    assert!(n.ech_outer);
    assert_eq!(r.counters().tls_ech, 1);
}

#[test]
fn malformed_records_and_invalid_names_are_counted() {
    let mut r = Rig::new();
    let mut bad = hello("x.example", false, false);
    let n = bad.len();
    bad[43] = 0xff; // corrupt the session id length
    bad.truncate(n);
    r.feed(&tcp_stream(addr(CLIENT), addr(SERVER), 1, &bad, 1460));
    assert_eq!(r.counters().tls_parse_error, 1);

    let mut r = Rig::new();
    r.feed(&tcp_stream(
        addr(CLIENT),
        addr(SERVER),
        1,
        &hello("bad name!", false, false),
        1460,
    ));
    assert!(r.name(addr(CLIENT), addr(SERVER), 6).is_none());
    assert_eq!(r.counters().name_invalid, 1);
}

#[test]
fn non_tls_payload_is_ignored() {
    let mut r = Rig::new();
    r.feed(&tcp_stream(
        addr(CLIENT),
        addr("1.2.3.4:22"),
        1,
        b"SSH-2.0-OpenSSH_9.6\r\n",
        1460,
    ));
    r.feed(&[fixtures::tcp(
        addr(CLIENT),
        addr("1.2.3.4:22"),
        30,
        ACK,
        b"more",
    )]);
    let c = r.counters();
    assert_eq!(
        (c.tls_started, c.tls_parse_error, c.tls_incomplete),
        (0, 0, 0)
    );
    assert_eq!(r.proc.pending_len(), 0);
}

#[test]
fn parser_helpers() {
    let rec = hello("h.example", false, false);
    assert!(tls::looks_like_client_hello(&rec));
    assert_eq!(tls::record_len(&rec), Some(rec.len()));
    assert_eq!(
        tls::from_record(&rec).unwrap().sni.as_deref(),
        Some("h.example")
    );
    assert!(tls::from_handshake(&rec[5..]).is_ok());
    assert_eq!(tls::handshake_len(&rec[5..]), Some(rec.len() - 5));
    assert!(tls::from_record(b"\x16\x03\x01\x00\x01\x02").is_err());
}
