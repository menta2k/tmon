//! The protocol loop over a real SOCK_SEQPACKET pair (feature 003, T039),
//! with the scripted engine.

mod common;

use common::*;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::thread;
use tmon_dpi::flows::Flows;
use tmon_dpi::ipc::{End, Seqpacket, Transport, hello, run};
use tmon_dpi_proto::{
    CatalogEntry, FromClassifier, Hello, MAX_MESSAGE, PROTO_VERSION, ToClassifier,
};

fn pair() -> (OwnedFd, OwnedFd) {
    let mut fds = [0; 2];
    // SAFETY: `fds` has room for the two descriptors socketpair writes.
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, fds.as_mut_ptr()) };
    assert_eq!(rc, 0);
    // SAFETY: both descriptors were just created and are owned here only.
    unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
}

/// tmon's end: blocking reads of whole messages.
struct Tmon(Seqpacket);

impl Tmon {
    fn send(&mut self, m: &ToClassifier) {
        self.0.send(&m.encode()).unwrap();
    }

    fn next(&mut self) -> FromClassifier {
        let mut buf = vec![0; MAX_MESSAGE + 1];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "no message from the classifier"
            );
            match self.0.recv(&mut buf).unwrap() {
                Some(n) if n > 0 => return FromClassifier::decode(&buf[..n]).unwrap(),
                Some(_) => panic!("classifier closed"),
                None => {}
            }
        }
    }

    /// Next message that is not a Stats heartbeat.
    fn next_reply(&mut self) -> FromClassifier {
        loop {
            match self.next() {
                FromClassifier::Stats(_) => {}
                m => return m,
            }
        }
    }
}

fn hello_msg() -> Hello {
    Hello {
        proto_version: PROTO_VERSION,
        impl_name: "tmon-dpi".into(),
        impl_version: "test".into(),
        engine: "mock".into(),
        max_flows: 4,
        catalog: vec![CatalogEntry {
            app: "BitTorrent".into(),
            category: "Download".into(),
        }],
    }
}

#[test]
fn hello_packets_ping_and_eof() {
    let (a, b) = pair();
    let side = thread::spawn(move || {
        // SAFETY: `a` is an open socket handed over to the transport.
        let mut t = unsafe { Seqpacket::from_fd(std::os::fd::IntoRawFd::into_raw_fd(a)) }.unwrap();
        hello(&mut t, &hello_msg()).unwrap();
        let mut flows = Flows::new(Mock::default(), limits());
        let end = run(&mut t, &mut flows).unwrap();
        (end, flows.counters())
    });
    // SAFETY: `b` is an open socket handed over to the transport.
    let mut tmon =
        Tmon(unsafe { Seqpacket::from_fd(std::os::fd::IntoRawFd::into_raw_fd(b)) }.unwrap());
    match tmon.next() {
        FromClassifier::Hello(h) => assert_eq!((h.proto_version, h.catalog.len()), (1, 1)),
        m => panic!("expected Hello, got {m:?}"),
    }
    // The first heartbeat goes out right away.
    assert!(matches!(tmon.next(), FromClassifier::Stats(_)));
    tmon.send(&ToClassifier::Packet(pkt(1, CONFIDENT, 1)));
    match tmon.next_reply() {
        FromClassifier::Classification(c) => {
            assert_eq!(
                (c.app.as_str(), c.final_, c.key),
                ("BitTorrent", true, key(1))
            )
        }
        m => panic!("expected Classification, got {m:?}"),
    }
    tmon.send(&ToClassifier::Ping(7));
    assert_eq!(tmon.next_reply(), FromClassifier::Pong(7));
    tmon.0.send(&[0xee, 1, 2, 3]).unwrap();
    tmon.send(&ToClassifier::Forget(key(1)));
    tmon.send(&ToClassifier::Ping(8));
    assert_eq!(tmon.next_reply(), FromClassifier::Pong(8));
    drop(tmon);
    let (end, c) = side.join().unwrap();
    assert_eq!(end, End::Eof);
    assert_eq!((c.packets_in, c.packets_invalid), (1, 1));
}

#[test]
fn peer_gone_while_sending_ends_cleanly() {
    let (a, b) = pair();
    let raw = a.as_raw_fd();
    std::mem::forget(a);
    drop(b);
    // SAFETY: `raw` is an open socket whose ownership moved to the transport.
    let mut t = unsafe { Seqpacket::from_fd(raw) }.unwrap();
    let mut flows = Flows::new(Mock::default(), limits());
    assert_eq!(run(&mut t, &mut flows).unwrap(), End::PeerGone);
}
