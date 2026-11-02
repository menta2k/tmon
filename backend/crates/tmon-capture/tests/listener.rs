//! Capture socket: allow-list and back-pressure (T057, T058).

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;
use tmon_capture::CaptureSettings;
use tmon_capture::fixtures::scenarios;
use tmon_capture::health::CounterValues;
use tmon_capture::listener::{self, CaptureHandle};
use tokio::sync::watch;

fn settings(allowed: &[&str], queue: usize) -> CaptureSettings {
    CaptureSettings {
        listen: "127.0.0.1:0".parse().unwrap(),
        allowed_sources: allowed.iter().map(|a| a.parse().unwrap()).collect(),
        queue_capacity: queue,
        ..CaptureSettings::default()
    }
}

fn send(to: SocketAddr, n: usize) {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    let d = &scenarios::datagrams("fasttrack_syn_only")[0];
    for _ in 0..n {
        s.send_to(d, to).unwrap();
    }
}

async fn wait(h: &CaptureHandle, pred: impl Fn(&CounterValues) -> bool) -> CounterValues {
    for _ in 0..100 {
        let c = h.health.counters.values();
        if pred(&c) {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("counters not reached: {:?}", h.health.counters.values());
}

fn started(allowed: &[&str]) -> (Arc<CaptureHandle>, SocketAddr, watch::Sender<bool>) {
    let (tx, rx) = watch::channel(false);
    let h = listener::start(settings(allowed, 64), rx);
    let addr = h.state().bound.expect("bound");
    (h, addr, tx)
}

#[tokio::test]
async fn sender_outside_the_allow_list_is_rejected_and_counted() {
    let (h, addr, _stop) = started(&["10.9.9.9"]);
    send(addr, 3);
    let c = wait(&h, |c| c.rejected_source >= 3).await;
    assert_eq!(c.datagrams_received, 0);
    assert_eq!(c.tcp_syn_seen, 0, "rejected datagrams are never decoded");
}

#[tokio::test]
async fn allowed_sender_is_processed() {
    let (h, addr, _stop) = started(&["127.0.0.1"]);
    send(addr, 2);
    let c = wait(&h, |c| c.datagrams_received >= 2).await;
    assert_eq!(c.rejected_source, 0);
    assert_eq!(c.tcp_syn_seen, 2);
}

#[tokio::test]
async fn empty_allow_list_accepts_every_sender() {
    let (h, addr, _stop) = started(&[]);
    assert!(h.accepts("203.0.113.7".parse::<IpAddr>().unwrap()));
    send(addr, 1);
    wait(&h, |c| c.datagrams_received >= 1).await;
}

#[test]
fn ipv4_mapped_ipv6_senders_match_ipv4_entries() {
    let h = CaptureHandle::new(settings(&["192.168.88.1"], 4), 0);
    assert!(h.accepts("::ffff:192.168.88.1".parse().unwrap()));
    assert!(!h.accepts("::ffff:192.168.88.2".parse().unwrap()));
}

#[tokio::test]
async fn full_queue_drops_and_counts_without_blocking_the_socket() {
    let h = CaptureHandle::new(settings(&[], 4), 0);
    let sock = listener::bind(h.settings.listen, 1 << 20).unwrap();
    let addr = sock.local_addr().unwrap();
    let (_stop, rx) = watch::channel(false);
    // Nobody drains the queue: the processor is stalled.
    let mut queue = listener::spawn_receiver(sock, h.clone(), rx);
    send(addr, 50);
    let c = wait(&h, |c| c.dropped_backpressure >= 46).await;
    assert_eq!(
        c.dropped_backpressure, 46,
        "exactly the queue capacity is kept"
    );
    // The socket task keeps receiving (and dropping) while the queue stays full.
    send(addr, 10);
    wait(&h, |c| c.dropped_backpressure >= 56).await;
    for _ in 0..4 {
        assert!(queue.try_recv().is_ok());
    }
    assert!(queue.try_recv().is_err());
}

#[tokio::test]
async fn oversize_datagrams_are_rejected_before_queueing() {
    let h = CaptureHandle::new(settings(&[], 4), 0);
    let sock = listener::bind(h.settings.listen, 1 << 20).unwrap();
    let addr = sock.local_addr().unwrap();
    let (_stop, rx) = watch::channel(false);
    let mut queue = listener::spawn_receiver(sock, h.clone(), rx);
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    let _ = s.send_to(&vec![0u8; listener::MAX_CAPTURE_DATAGRAM + 1], addr);
    send(addr, 1); // probe: once it is queued, the oversize one was handled
    let probe = loop {
        if let Ok(d) = queue.try_recv() {
            break d;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(probe.len() <= listener::MAX_CAPTURE_DATAGRAM);
    assert!(
        queue.try_recv().is_err(),
        "the oversize datagram was queued"
    );
    if h.health.counters.values().malformed_tzsp == 0 {
        eprintln!(
            "host network dropped the oversize datagram (WSL2 loopback limit); rejection path not exercised"
        );
    }
}
