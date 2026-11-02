//! Application classification core types (data-model.md §1–2).

use std::sync::Arc;
use tmon_core::app::{AppError, AppInfo, GENERIC_CARRIERS, is_generic_carrier, valid_app_name};
use tmon_core::{Direction, FlowRecord, NameSource, RawFlow};

fn info(app: &str, confident: bool) -> AppInfo {
    AppInfo::new(app, "VPN", confident, None, false).unwrap()
}

#[test]
fn validates_names() {
    assert!(valid_app_name("BitTorrent"));
    assert!(valid_app_name(&"a".repeat(48)));
    assert!(!valid_app_name(&"a".repeat(49)));
    assert!(!valid_app_name(""));
    assert!(!valid_app_name("   "));
    assert!(!valid_app_name("Bit\u{7}"));
    assert!(!valid_app_name("Zoom™"));
    assert_eq!(
        AppInfo::new("", "VPN", true, None, false),
        Err(AppError::InvalidApp)
    );
    assert_eq!(
        AppInfo::new("WireGuard", &"c".repeat(49), true, None, false),
        Err(AppError::InvalidCategory)
    );
    assert!(AppInfo::new("WireGuard", "", true, None, false).is_ok());
}

#[test]
fn rejects_generic_carriers_case_insensitively() {
    for g in GENERIC_CARRIERS {
        assert!(is_generic_carrier(&g.to_ascii_lowercase()));
        assert_eq!(
            AppInfo::new(g, "Web", true, None, false),
            Err(AppError::GenericCarrier)
        );
    }
    assert!(!is_generic_carrier("Zoom"));
}

#[test]
fn host_is_normalized_or_dropped() {
    let a = AppInfo::new("Zoom", "Video", true, Some("Zoom.US."), true).unwrap();
    assert_eq!(a.host.as_deref(), Some("zoom.us"));
    let b = AppInfo::new("Zoom", "Video", true, Some("bad host!"), true).unwrap();
    assert_eq!(b.host, None);
}

#[test]
fn guess_never_replaces_confident() {
    let confident = info("WireGuard", true);
    let guess = info("OpenVPN", false);
    assert!(!guess.better_than(&confident));
    assert!(confident.better_than(&guess));
    assert!(!confident.better_than(&confident.clone()));
    assert!(info("Steam", true).better_than(&confident));
    let final_ = AppInfo {
        final_: true,
        ..confident.clone()
    };
    assert!(final_.better_than(&confident));
}

#[test]
fn name_source_dpi_round_trips() {
    assert_eq!(NameSource::Dpi.code(), 6);
    assert_eq!(NameSource::from_code(6), Some(NameSource::Dpi));
    assert_eq!(NameSource::parse("dpi"), Some(NameSource::Dpi));
    assert_eq!(NameSource::Dpi.as_str(), "dpi");
    assert_eq!(NameSource::from_code(7), None);
}

fn record() -> FlowRecord {
    let raw = RawFlow {
        ts_ms: 1,
        ts_start_ms: 1,
        exporter_id: 1,
        in_if: 1,
        out_if: 2,
        src_ip: "192.168.1.10".parse().unwrap(),
        dst_ip: "8.8.8.8".parse().unwrap(),
        src_port: 50000,
        dst_port: 51820,
        proto: 17,
        tcp_flags: 0,
        bytes: 100,
        packets: 1,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    };
    FlowRecord::from_raw(&raw, Direction::Up, true)
}

#[test]
fn with_app_copies_without_mutating() {
    let f = record();
    let g = f.with_app(&info("WireGuard", true));
    assert_eq!(f.app, None);
    assert!(!f.app_confident);
    assert_eq!(g.app.as_deref(), Some("WireGuard"));
    assert!(g.app_confident);
    assert_eq!(g.app_category.as_deref(), Some("VPN"));
    let empty_cat = AppInfo::new("X", "", false, None, false).unwrap();
    assert_eq!(f.with_app(&empty_cat).app_category, None);
}

#[test]
fn stored_app_without_name_is_never_confident() {
    let f = record().with_stored_app(None, Some(true));
    assert!(!f.app_confident);
    let g = record().with_stored_app(Some(Arc::from("Steam")), None);
    assert!(!g.app_confident);
    let h = record().with_stored_app(Some(Arc::from("Steam")), Some(true));
    assert!(h.app_confident);
}
