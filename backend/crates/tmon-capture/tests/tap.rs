//! Classifier tap and reply orientation (feature 003, research R4/R5).

mod common;

use common::{NOW, Rig};
use std::sync::{Arc, Mutex};
use tmon_capture::fixtures::{ACK, addr, hello, tcp, tcp_ip, tcp_stream, udp, udp_ip};
use tmon_capture::{PacketTap, TapConfig, TapDir};
use tmon_core::LocalNetworks;
use tmon_core::naming::ConnKey;

type Offer = (ConnKey, TapDir, i64, Vec<u8>);

#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<Offer>>,
}

impl PacketTap for Recorder {
    fn offer(&self, key: &ConnKey, dir: TapDir, ts_ms: i64, l3: &[u8]) {
        self.seen
            .lock()
            .unwrap()
            .push((*key, dir, ts_ms, l3.to_vec()));
    }
}

fn rig() -> (Rig, Arc<Recorder>) {
    let rec = Arc::new(Recorder::default());
    let rig = Rig::new().with_tap(TapConfig::with_networks(
        rec.clone(),
        LocalNetworks::defaults(),
    ));
    (rig, rec)
}

const CLIENT: &str = "192.168.88.10:50000";
const SERVER: &str = "203.0.113.5:6881";

#[test]
fn reply_packets_share_the_lan_side_key() {
    let (mut rig, rec) = rig();
    let (c, s) = (addr(CLIENT), addr(SERVER));
    rig.feed(&[
        tcp(c, s, 1, ACK, b"\x13BitTorrent protocol"),
        tcp(s, c, 9, ACK, b"\x13BitTorrent protocol"),
    ]);
    let seen = rec.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].0, seen[1].0);
    assert_eq!(seen[0].0.lan_ip, c.ip());
    assert_eq!(seen[0].0.remote_port, 6881);
    assert_eq!(seen[0].1, TapDir::LanToRemote);
    assert_eq!(seen[1].1, TapDir::RemoteToLan);
    assert_eq!(seen[0].2, NOW);
}

#[test]
fn tap_gets_the_exact_ip_packet() {
    let (mut rig, rec) = rig();
    let (c, s) = (addr(CLIENT), addr("[2001:db8::5]:443"));
    let c6 = addr("[fd00::10]:50000");
    rig.feed(&[udp(
        c,
        addr("203.0.113.9:27015"),
        b"\xff\xff\xff\xffTSource Engine Query\0",
    )]);
    rig.feed(&[tcp(c6, s, 7, ACK, b"hello")]);
    let seen = rec.seen.lock().unwrap();
    assert_eq!(
        seen[0].3,
        udp_ip(
            c,
            addr("203.0.113.9:27015"),
            b"\xff\xff\xff\xffTSource Engine Query\0"
        )
    );
    assert_eq!(seen[1].3, tcp_ip(c6, s, 7, ACK, b"hello"));
}

#[test]
fn reply_packets_do_not_affect_naming() {
    let (mut rig, rec) = rig();
    let (c, s) = (addr("192.168.88.10:50001"), addr("142.250.1.1:443"));
    let ch = hello::record(&hello::client_hello(&hello::Hello {
        sni: Some("www.example.com"),
        ..hello::Hello::default()
    }));
    // A server "ClientHello" in the reply direction must not name anything.
    rig.feed(&tcp_stream(s, c, 100, &ch, 1400));
    let before = rig.counters();
    assert_eq!(rig.name(s, c, 6), None);
    assert_eq!(rig.name(c, s, 6), None);
    assert_eq!(before.tls_named, 0);
    // The client's own hello still names the connection.
    rig.feed(&tcp_stream(c, s, 1, &ch, 1400));
    assert_eq!(rig.name(c, s, 6).unwrap().host.as_ref(), "www.example.com");
    assert!(rec.seen.lock().unwrap().len() >= 4);
}

#[test]
fn dns_is_never_tapped_and_still_names() {
    let (mut rig, rec) = rig();
    let (c, r) = (addr("192.168.88.10:40000"), addr("192.168.88.1:53"));
    rig.feed(&[udp(
        c,
        r,
        &tmon_capture::fixtures::dns::query("example.com"),
    )]);
    let resolver = addr("9.9.9.9:53");
    rig.feed(&[udp(
        c,
        resolver,
        &tmon_capture::fixtures::dns::query("example.com"),
    )]);
    assert!(rec.seen.lock().unwrap().is_empty());
}

#[test]
fn without_a_tap_the_source_is_the_lan_side() {
    // Feature 002 behaviour: the sender is treated as the client.
    let mut rig = Rig::new();
    let (c, s) = (addr("203.0.113.5:50000"), addr("142.250.1.1:443"));
    let ch = hello::record(&hello::client_hello(&hello::Hello {
        sni: Some("www.example.com"),
        ..hello::Hello::default()
    }));
    rig.feed(&tcp_stream(c, s, 1, &ch, 1400));
    assert!(rig.name(c, s, 6).is_some());
}
