//! The application fixtures reach the classifier tap in both directions
//! under one LAN-side key (feature 003, T035).

mod common;

use common::Rig;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tmon_capture::fixtures::addr;
use tmon_capture::fixtures::apps::{self, APPS, PORT_ONLY, ZOOM};
use tmon_capture::{PacketTap, TapConfig, TapDir};
use tmon_core::LocalNetworks;
use tmon_core::naming::ConnKey;

#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<(ConnKey, TapDir)>>,
}

impl PacketTap for Recorder {
    fn offer(&self, key: &ConnKey, dir: TapDir, _ts: i64, _l3: &[u8]) {
        self.seen.lock().unwrap().push((*key, dir));
    }
}

fn tapped(datagrams: &[Vec<u8>]) -> Vec<(ConnKey, TapDir)> {
    let rec = Arc::new(Recorder::default());
    let mut rig = Rig::new().with_tap(TapConfig::with_networks(
        rec.clone(),
        LocalNetworks::defaults(),
    ));
    rig.feed(datagrams);
    rec.seen.lock().unwrap().clone()
}

#[test]
fn every_app_scenario_is_seen_in_both_directions() {
    for e in APPS {
        let seen = tapped(&apps::datagrams(e));
        let keys: HashSet<ConnKey> = seen.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys.len(), 1, "{}: one connection", e.scenario);
        let key = keys.into_iter().next().unwrap();
        assert_eq!(key.lan_ip, addr(e.client).ip(), "{}", e.scenario);
        assert_eq!(key.remote_port, addr(e.server).port(), "{}", e.scenario);
        assert_eq!(key.proto, e.proto, "{}", e.scenario);
        for dir in [TapDir::LanToRemote, TapDir::RemoteToLan] {
            assert!(
                seen.iter().any(|(_, d)| *d == dir),
                "{}: {dir:?}",
                e.scenario
            );
        }
    }
}

#[test]
fn mix_contains_every_scenario() {
    let seen = tapped(&apps::mix());
    let keys: HashSet<ConnKey> = seen.iter().map(|(k, _)| *k).collect();
    assert_eq!(keys.len(), APPS.len());
}

#[test]
fn zoom_and_port_only_are_one_way_connections() {
    for e in [ZOOM, PORT_ONLY] {
        let seen = tapped(&apps::datagrams(&e));
        assert!(!seen.is_empty(), "{}", e.scenario);
        assert!(seen.iter().all(|(_, d)| *d == TapDir::LanToRemote));
    }
}
