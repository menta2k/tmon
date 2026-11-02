use std::sync::Arc;
use std::time::Duration;
use tmon_collector::fixtures::corpus::{BASE_MS, basic_flows};
use tmon_collector::fixtures::encode_ipfix;
use tmon_collector::listener::{ListenerCtx, ListenerKind, bind, spawn};
use tmon_collector::{DropReason, ExporterRegistry, GlobalCounters};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};

async fn setup(
    allowed: Vec<std::net::IpAddr>,
    cap: usize,
) -> (
    std::net::SocketAddr,
    ListenerCtx,
    mpsc::Receiver<Vec<tmon_core::RawFlow>>,
    watch::Sender<bool>,
) {
    let sock = bind("127.0.0.1:0".parse().unwrap(), 1 << 20).unwrap();
    let addr = sock.local_addr().unwrap();
    let (tx, rx) = mpsc::channel(cap);
    let ctx = ListenerCtx {
        registry: Arc::new(ExporterRegistry::new()),
        globals: Arc::new(GlobalCounters::default()),
        tx,
        allowed: Arc::new(allowed),
        new_exporters: None,
    };
    let (stop, stop_rx) = watch::channel(false);
    spawn(sock, ListenerKind::Netflow, ctx.clone(), stop_rx);
    (addr, ctx, rx, stop)
}

async fn eventually(f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached");
}

#[tokio::test]
async fn drops_datagrams_from_non_allowed_exporters() {
    let (addr, ctx, mut rx, _stop) = setup(vec!["10.9.9.9".parse().unwrap()], 16).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let msg = encode_ipfix(BASE_MS, 1, 0, true, &basic_flows());
    client.send_to(&msg, addr).await.unwrap();
    eventually(|| ctx.globals.dropped(DropReason::NotAllowed) == 1).await;
    assert!(rx.try_recv().is_err());
    assert!(ctx.registry.all().is_empty());
}

#[tokio::test]
async fn backpressure_drops_and_keeps_running() {
    let (addr, ctx, mut rx, _stop) = setup(vec![], 1).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let msg = encode_ipfix(BASE_MS, 1, 0, true, &basic_flows());
    for _ in 0..5 {
        client.send_to(&msg, addr).await.unwrap();
    }
    eventually(|| ctx.globals.dropped(DropReason::Backpressure) == 4 * 4).await;
    assert_eq!(rx.recv().await.unwrap().len(), 4);
    client.send_to(&msg, addr).await.unwrap();
    let batch = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(batch.len(), 4, "listener still running after drops");
    assert_eq!(ctx.registry.all()[0].counters.snapshot().received, 6);
}

#[tokio::test]
async fn unknown_versions_are_counted() {
    let (addr, ctx, _rx, stop) = setup(vec![], 4).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.send_to(&[0x42, 0x42, 0, 0], addr).await.unwrap();
    eventually(|| ctx.globals.dropped(DropReason::UnknownVersion) == 1).await;
    stop.send(true).unwrap();
}
