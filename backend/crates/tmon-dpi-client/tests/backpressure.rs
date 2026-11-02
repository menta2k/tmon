//! A slow classifier never blocks the capture path (FR-018, T071).

mod common;

use common::*;
use std::time::{Duration, Instant};
use tmon_capture::{PacketTap, TapDir};
use tmon_dpi_client::DpiState;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_classifier_drops_at_the_queue_without_blocking() {
    let mut s = settings(&["--mode", "slow_us=2000"]);
    s.queue_capacity = 256;
    s.queue_max_bytes = 1 << 20;
    s.max_packets_per_conn = 255;
    let r = start(s, fast());
    assert!(
        wait_for(Duration::from_secs(5), || r.handle.health.state()
            == DpiState::Running)
        .await
    );
    let tap = &r.handle.tap;
    let mut worst = Duration::ZERO;
    for i in 0..50_000u32 {
        let k = key((i % 60_000) as u16);
        let pkt = l3(&k);
        let t = Instant::now();
        tap.offer(&k, TapDir::LanToRemote, now(), &pkt);
        worst = worst.max(t.elapsed());
        assert!(tap.queued_bytes() <= 1 << 20);
    }
    let v = tap.counters.values();
    assert!(v.dropped_queue > 0, "{v:?}");
    assert!(v.sent + v.dropped_queue + v.skipped_final >= 50_000);
    // try_send only: even the slowest offer is far below a millisecond.
    assert!(worst < Duration::from_millis(5), "worst offer {worst:?}");
    r.shutdown().await;
}
