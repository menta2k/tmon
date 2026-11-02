//! Tap accounting (review fixes): packets that never reach the queue do not
//! use up a connection's budget, and queued bytes balance out exactly.

mod common;

use common::*;
use tmon_capture::{PacketTap, TapDir};
use tmon_dpi_client::tap::DpiTap;

#[test]
fn packets_dropped_while_down_do_not_use_the_budget() {
    let (tap, mut rx) = DpiTap::new(64, 1 << 20, 4, 100);
    let k = key(443);
    for _ in 0..10 {
        tap.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
    }
    assert_eq!(tap.counters.values().dropped_down, 10);
    tap.set_running(true);
    for _ in 0..10 {
        tap.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
    }
    let v = tap.counters.values();
    assert_eq!((v.sent, v.skipped_final), (4, 6));
    let mut n = 0;
    while let Ok(m) = rx.try_recv() {
        tap.dequeued(m.len());
        n += 1;
    }
    assert_eq!((n, tap.queued_bytes()), (4, 0));
}

#[test]
fn byte_budget_drops_without_charging_and_dequeue_never_underflows() {
    let k = key(443);
    let one = {
        let (probe, mut rx) = DpiTap::new(4, 1 << 20, 32, 100);
        probe.set_running(true);
        probe.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
        rx.try_recv().unwrap().len()
    };
    let (tap, _rx) = DpiTap::new(64, one, 32, 100);
    tap.set_running(true);
    tap.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
    tap.offer(&k, TapDir::LanToRemote, now(), &l3(&k));
    let v = tap.counters.values();
    assert_eq!((v.sent, v.dropped_queue), (1, 1));
    assert_eq!(tap.queued_bytes(), one);
    tap.dequeued(one * 5);
    assert_eq!(tap.queued_bytes(), 0);
}
