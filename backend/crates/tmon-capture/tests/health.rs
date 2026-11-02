//! Health detectors at the R10 thresholds (T048).

use std::time::Duration;
use tmon_capture::CaptureStats;
use tmon_capture::health::{
    Bucket, CaptureWarning, CounterValues, FASTPATH_MAX_PAYLOAD_RATIO, FASTPATH_MIN_SYNS, Health,
    LARGE_LOSS_MAX_DATAGRAM, LARGE_LOSS_MIN_INCOMPLETE_RATIO, LARGE_LOSS_MIN_TLS, warnings,
};

const T0: i64 = 1_700_000_000_000;
const SILENT: Duration = Duration::from_secs(300);

fn stats(window: Bucket, last: Option<i64>) -> CaptureStats {
    CaptureStats {
        address: "0.0.0.0:37008".parse().unwrap(),
        listening: true,
        error: None,
        counters: CounterValues::default(),
        window,
        last_datagram_ms: last,
        conns_tracked: 0,
        bindings_tracked: 0,
        reassembly_bytes: 0,
        started_ms: T0,
    }
}

fn healthy_window() -> Bucket {
    Bucket {
        max_datagram_len: 1518,
        ..Bucket::default()
    }
}

fn at(now: i64, s: &CaptureStats) -> Vec<CaptureWarning> {
    warnings(s, SILENT, now)
}

#[test]
fn thresholds_match_research_r10() {
    assert_eq!(LARGE_LOSS_MIN_TLS, 50);
    assert_eq!(LARGE_LOSS_MIN_INCOMPLETE_RATIO, 0.5);
    assert_eq!(LARGE_LOSS_MAX_DATAGRAM, 1472);
    assert_eq!(FASTPATH_MIN_SYNS, 100);
    assert_eq!(FASTPATH_MAX_PAYLOAD_RATIO, 0.1);
}

#[test]
fn silent_after_300s_since_last_datagram() {
    let last = T0 + 10_000;
    let s = stats(healthy_window(), Some(last));
    assert!(at(last + 299_999, &s).is_empty());
    assert!(
        at(last + 300_000, &s).is_empty(),
        "exactly 300 s is not yet silent"
    );
    assert_eq!(at(last + 300_001, &s), vec![CaptureWarning::Silent]);
}

#[test]
fn silent_when_nothing_ever_arrived_counts_from_start() {
    let s = stats(Bucket::default(), None);
    assert!(at(T0 + 1_000, &s).is_empty());
    assert_eq!(at(T0 + 300_001, &s), vec![CaptureWarning::Silent]);
}

#[test]
fn not_listening_reports_the_bind_error_only() {
    let mut s = stats(Bucket::default(), None);
    s.listening = false;
    s.error = Some("address in use".into());
    assert_eq!(
        at(T0 + 900_000, &s),
        vec![CaptureWarning::ListenerDown { port: 37008 }]
    );
}

fn wsl(tls_started: u64, tls_incomplete: u64, max_len: u64) -> Bucket {
    Bucket {
        tls_started,
        tls_incomplete,
        max_datagram_len: max_len,
        ..Bucket::default()
    }
}

#[test]
fn large_packet_loss_at_the_exact_thresholds() {
    let now = T0 + 1_000;
    let fire = |b| at(now, &stats(b, Some(now))).contains(&CaptureWarning::LargePacketsLost);
    assert!(fire(wsl(50, 25, 1472)));
    assert!(fire(wsl(60, 60, 1200)));
    assert!(!fire(wsl(49, 49, 1472)), "49 starts is below the minimum");
    assert!(!fire(wsl(50, 24, 1472)), "48 % incomplete");
    assert!(
        !fire(wsl(50, 50, 1473)),
        "a 1,473-byte datagram proves large ones get through"
    );
}

fn ft(syn: u64, first: u64) -> Bucket {
    Bucket {
        tcp_syn: syn,
        tcp_payload_first: first,
        max_datagram_len: 1518,
        ..Bucket::default()
    }
}

#[test]
fn fastpath_at_the_exact_thresholds() {
    let now = T0 + 1_000;
    let fire = |b| at(now, &stats(b, Some(now))).contains(&CaptureWarning::FastpathSuspected);
    assert!(fire(ft(100, 9)));
    assert!(fire(ft(120, 0)));
    assert!(!fire(ft(99, 0)), "99 SYNs is below the minimum");
    assert!(!fire(ft(100, 10)), "exactly 10 % is healthy");
}

#[test]
fn window_rolls_off_buckets_older_than_15_minutes() {
    let h = Health::default();
    for i in 0..60 {
        h.record(T0 + i, |b| {
            b.tls_started += 1;
            b.tls_incomplete += 1;
        });
    }
    h.datagram(T0, 1200);
    assert_eq!(h.window(T0).tls_started, 60);
    let s = |now| stats(h.window(now), Some(now));
    let now = T0 + 14 * 60_000;
    assert!(at(now, &s(now)).contains(&CaptureWarning::LargePacketsLost));
    let later = T0 + 15 * 60_000;
    assert_eq!(h.window(later).tls_started, 0);
    assert!(at(later, &s(later)).is_empty());
}

#[test]
fn codes_and_messages() {
    for (w, code) in [
        (CaptureWarning::Silent, "capture_silent"),
        (
            CaptureWarning::LargePacketsLost,
            "capture_large_packets_lost",
        ),
        (
            CaptureWarning::FastpathSuspected,
            "capture_fastpath_suspected",
        ),
        (
            CaptureWarning::ListenerDown { port: 37008 },
            "collector_down:37008",
        ),
    ] {
        assert_eq!(w.code(), code);
        assert!(!w.message().is_empty());
    }
}
