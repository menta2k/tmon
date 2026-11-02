mod common;

use common::*;
use tmon_capture::fixtures::hello::{Hello, client_hello};
use tmon_capture::fixtures::quic::{client_initial, crypto_frame};
use tmon_capture::fixtures::{addr, udp};
use tmon_capture::quic::Version;
use tmon_core::naming::NameSource;

const CLIENT: &str = "192.168.88.10:55000";
const SERVER: &str = "142.250.1.1:443";
const DCID: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

fn hello(sni: &str) -> Vec<u8> {
    client_hello(&Hello {
        sni: Some(sni),
        alpn: &["h3"],
        ech: false,
        pq_key_share: true,
    })
}

/// The hello split into `parts` CRYPTO frames, one Initial packet each.
fn initials(v: Version, hello: &[u8], parts: usize, extra: &[u8]) -> Vec<Vec<u8>> {
    let size = hello.len().div_ceil(parts);
    hello
        .chunks(size)
        .enumerate()
        .map(|(i, c)| {
            let mut frames = extra.to_vec();
            frames.extend(crypto_frame((i * size) as u64, c));
            client_initial(v, &DCID, &[9, 9], i as u32, &frames, 1162)
        })
        .collect()
}

fn datagrams(pkts: &[Vec<u8>]) -> Vec<Vec<u8>> {
    pkts.iter()
        .map(|p| udp(addr(CLIENT), addr(SERVER), p))
        .collect()
}

#[test]
fn crypto_split_over_initials_in_reverse_order() {
    for v in [Version::V1, Version::V2] {
        let mut pkts = initials(v, &hello("www.youtube.com"), 3, &[]);
        pkts.reverse();
        let mut r = Rig::new();
        r.feed(&datagrams(&pkts));
        let n = r
            .name(addr(CLIENT), addr(SERVER), 17)
            .unwrap_or_else(|| panic!("{v:?}"));
        assert_eq!(
            (&*n.host, n.source, n.alpn.as_deref()),
            ("www.youtube.com", NameSource::Quic, Some("h3"))
        );
        let c = r.counters();
        assert_eq!(
            (c.quic_started, c.quic_named, c.quic_decrypt_error),
            (1, 1, 0)
        );
        assert_eq!(r.proc.reassembly_bytes(), 0);
    }
}

#[test]
fn coalesced_packets_and_ack_ping_padding_frames() {
    // ACK (largest 0, delay 0, 0 ranges, first 0), PING, PADDING.
    let extra = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00];
    let pkts = initials(Version::V1, &hello("chat.example"), 2, &extra);
    let mut r = Rig::new();
    r.feed(&datagrams(&[pkts.concat()]));
    assert_eq!(
        &*r.name(addr(CLIENT), addr(SERVER), 17).unwrap().host,
        "chat.example"
    );
}

#[test]
fn unknown_versions_and_bad_packets_are_counted() {
    let mut r = Rig::new();
    let mut unknown = client_initial(Version::V1, &DCID, &[], 0, &crypto_frame(0, b"x"), 1162);
    unknown[1..5].copy_from_slice(&0xff00_001du32.to_be_bytes());
    r.feed(&datagrams(&[unknown]));
    assert_eq!(r.counters().quic_unsupported_version, 1);

    let mut r = Rig::new();
    let mut bad = initials(Version::V1, &hello("x.example"), 1, &[]).remove(0);
    let last = bad.len() - 1;
    bad[last] ^= 0x55;
    r.feed(&datagrams(&[bad]));
    let c = r.counters();
    assert_eq!((c.quic_decrypt_error, c.quic_started), (1, 0));
    assert!(r.name(addr(CLIENT), addr(SERVER), 17).is_none());
}

#[test]
fn incomplete_hello_times_out() {
    let pkts = initials(Version::V1, &hello("half.example"), 2, &[]);
    let mut r = Rig::new();
    r.feed(&datagrams(&pkts[..1]));
    assert_eq!(r.counters().quic_started, 1);
    r.proc.handle(&datagrams(&pkts[..1])[0], NOW + 11_000);
    assert_eq!(r.counters().quic_incomplete, 1);
}

#[test]
fn short_headers_and_random_udp_are_ignored() {
    let mut r = Rig::new();
    r.feed(&datagrams(&[vec![0x40, 1, 2, 3], vec![], vec![0xc0, 0, 0]]));
    let c = r.counters();
    assert_eq!(
        (
            c.quic_started,
            c.quic_decrypt_error,
            c.quic_unsupported_version
        ),
        (0, 0, 0)
    );
}
