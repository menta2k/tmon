//! Flow table behaviour (feature 003, T028), with a scripted engine.

mod common;

use common::*;
use tmon_dpi::flows::{Flows, Limits};

fn flows() -> (Flows<Mock>, std::rc::Rc<std::cell::Cell<u64>>) {
    let m = Mock::default();
    let live = m.live.clone();
    (Flows::new(m, limits()), live)
}

#[test]
fn lru_bounds_the_table() {
    let (mut f, live) = flows();
    for port in 0..10 {
        f.packet(&pkt(port, UNKNOWN, 1_000));
    }
    assert_eq!(f.active(), 4);
    assert_eq!(live.get(), 4, "evicted flows are freed");
    let c = f.counters();
    assert_eq!((c.flows_created, c.evicted_lru), (10, 6));
}

#[test]
fn idle_flows_are_swept() {
    let (mut f, live) = flows();
    f.packet(&pkt(1, UNKNOWN, 1_000));
    f.packet(&pkt(2, UNKNOWN, 50_000));
    f.sweep(61_000);
    assert_eq!((f.active(), live.get()), (1, 1));
    assert_eq!(f.counters().evicted_idle, 1);
    f.sweep(200_000);
    assert_eq!((f.active(), f.counters().evicted_idle), (0, 2));
}

#[test]
fn classification_is_sent_only_on_change() {
    let mut f = Flows::new(
        Mock::default(),
        Limits {
            max_packets: 10,
            ..limits()
        },
    );
    assert!(f.packet(&pkt(1, UNKNOWN, 1)).is_none());
    let first = f.packet(&pkt(1, GUESS, 2)).expect("first guess");
    assert_eq!(
        (first.app.as_str(), first.confident, first.final_),
        ("WireGuard", false, false)
    );
    for ts in 3..8 {
        assert!(
            f.packet(&pkt(1, GUESS, ts)).is_none(),
            "unchanged verdict at {ts}"
        );
    }
    let upgrade = f.packet(&pkt(1, CONFIDENT, 9)).expect("changed verdict");
    assert_eq!(
        (upgrade.app.as_str(), upgrade.confident, upgrade.final_),
        ("BitTorrent", true, true)
    );
    let c = f.counters();
    assert_eq!((c.classified_guess, c.classified_confident), (1, 1));
}

#[test]
fn confident_result_is_final_and_frees_the_flow() {
    let (mut f, live) = flows();
    let c = f.packet(&pkt(1, CONFIDENT, 1)).unwrap();
    assert!(c.confident && c.final_);
    assert_eq!((f.active(), live.get()), (0, 0));
    // Packets after final are ignored, not re-classified.
    assert!(f.packet(&pkt(1, CONFIDENT, 2)).is_none());
    assert_eq!(f.counters().flows_created, 1);
    assert_eq!(f.stats(0).top[0].app, "BitTorrent");
}

#[test]
fn packet_budget_gives_up_with_a_final_guess() {
    let (mut f, live) = flows();
    assert!(f.packet(&pkt(1, UNKNOWN, 1)).is_none());
    assert!(f.packet(&pkt(1, UNKNOWN, 2)).is_none());
    let c = f.packet(&pkt(1, UNKNOWN, 3)).expect("giveup verdict");
    assert_eq!(
        (c.app.as_str(), c.confident, c.final_),
        ("WireGuard", false, true)
    );
    assert_eq!((f.counters().giveups, live.get()), (1, 0));
}

#[test]
fn guess_then_giveup_resends_as_final() {
    let (mut f, _) = flows();
    assert!(!f.packet(&pkt(1, GUESS, 1)).unwrap().final_);
    assert!(f.packet(&pkt(1, GUESS, 2)).is_none());
    let last = f.packet(&pkt(1, GUESS, 3)).expect("final");
    assert!(last.final_ && !last.confident);
    assert_eq!(f.counters().classified_guess, 1);
}

#[test]
fn carriers_are_not_reported() {
    let (mut f, _) = flows();
    assert!(f.packet(&pkt(1, CARRIER, 1)).is_none());
    assert_eq!(f.counters().classified_confident, 0);
}

#[test]
fn forget_frees_the_flow_and_the_final_marker() {
    let (mut f, live) = flows();
    f.packet(&pkt(1, UNKNOWN, 1));
    f.forget(&key(1));
    assert_eq!((f.active(), live.get()), (0, 0));
    f.packet(&pkt(2, CONFIDENT, 1));
    f.forget(&key(2));
    assert!(
        f.packet(&pkt(2, CONFIDENT, 2)).is_some(),
        "re-classified after Forget"
    );
}

#[test]
fn memory_budget_evicts_lru_flows_to_low_water() {
    let m = Mock::default();
    let live = m.live.clone();
    let lim = Limits {
        max_flows: 100,
        mem_limit_bytes: 10 * FLOW_BYTES,
        ..limits()
    };
    let mut f = Flows::new(m, lim);
    for port in 0..11 {
        f.packet(&pkt(port, UNKNOWN, 1));
    }
    assert!(live.get() * FLOW_BYTES <= 9 * FLOW_BYTES, "{}", live.get());
    assert!(f.counters().evicted_memory >= 2);
    let s = f.stats(5);
    assert_eq!(
        (s.mem_limit_bytes, s.flows_active as u64),
        (10 * FLOW_BYTES, live.get())
    );
}

#[test]
fn stats_report_counters_and_bounded_top_apps() {
    let lim = Limits {
        max_flows: 100,
        ..limits()
    };
    let mut f = Flows::new(Mock::default(), lim);
    for port in 0..20 {
        f.packet(&pkt(port, if port % 2 == 0 { CONFIDENT } else { GUESS }, 1));
    }
    f.invalid();
    let s = f.stats(42);
    assert_eq!((s.uptime_ms, s.packets_in, s.packets_invalid), (42, 20, 1));
    assert_eq!((s.classified_confident, s.classified_guess), (10, 10));
    assert_eq!(s.top.len(), 2);
    assert_eq!(s.top[0].flows, 10);
}

#[test]
fn stats_report_flow_memory_above_the_engine_baseline() {
    let m = Mock::default();
    let live = m.live.clone();
    live.set(5); // the detection module's own allocations
    let lim = Limits {
        max_flows: 100,
        mem_limit_bytes: 10 * FLOW_BYTES,
        ..limits()
    };
    let mut f = Flows::new(m, lim);
    for port in 0..3 {
        f.packet(&pkt(port, UNKNOWN, 1));
    }
    let s = f.stats(5);
    // Same quantity the budget limits, so mem_bytes <= mem_limit_bytes holds.
    assert_eq!(s.mem_bytes, 3 * FLOW_BYTES);
}
