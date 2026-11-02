//! Real nDPI on the application fixtures (feature 003, T029 / SC-002).
#![cfg(feature = "ndpi")]

mod real;

use real::{Sidecar, packets};
use std::time::Duration;
use tmon_capture::fixtures::apps::{APPS, PORT_ONLY, ZOOM};

const QUIET: Duration = Duration::from_millis(500);

#[test]
fn hello_lists_a_catalog_without_carriers() {
    let s = Sidecar::spawn(&[]);
    assert_eq!(s.hello.proto_version, 1);
    assert!(s.hello.engine.starts_with("nDPI"), "{}", s.hello.engine);
    let apps: Vec<&str> = s.hello.catalog.iter().map(|e| e.app.as_str()).collect();
    for a in ["BitTorrent", "WireGuard", "OpenVPN", "STUN", "RDP", "Zoom"] {
        assert!(apps.contains(&a), "{a} missing from catalog");
    }
    assert!(
        !apps
            .iter()
            .any(|a| ["TLS", "QUIC", "HTTP", "DNS", "Unknown"].contains(a))
    );
}

#[test]
fn fixtures_are_classified_confidently() {
    let mut s = Sidecar::spawn(&[]);
    let mut hits = 0;
    let mut misses = Vec::new();
    for e in APPS {
        for p in packets(e) {
            s.send(&p);
        }
        let got = s.classifications(QUIET);
        let ok = got
            .iter()
            .any(|c| c.app.eq_ignore_ascii_case(e.app) && c.confident);
        if ok {
            hits += 1;
        } else {
            misses.push(format!("{}: {:?}", e.scenario, got));
        }
    }
    // SC-002: at least 90 % of the fixture connections.
    assert!(
        hits * 10 >= APPS.len() * 9,
        "{hits}/{} — misses: {misses:#?}",
        APPS.len()
    );
}

#[test]
fn zoom_tls_reports_zoom() {
    let mut s = Sidecar::spawn(&[]);
    for p in packets(&ZOOM) {
        s.send(&p);
    }
    let got = s.classifications(QUIET);
    assert!(got.iter().any(|c| c.app == "Zoom"), "{got:?}");
}

#[test]
fn non_wireguard_payload_on_51820_is_not_a_confident_wireguard() {
    let mut s = Sidecar::spawn(&[]);
    for p in packets(&PORT_ONLY) {
        s.send(&p);
    }
    let got = s.classifications(QUIET);
    assert!(
        !got.iter().any(|c| c.app == "WireGuard" && c.confident),
        "{got:?}"
    );
}
