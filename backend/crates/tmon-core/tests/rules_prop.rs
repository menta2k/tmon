use proptest::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use tmon_core::rules::{Origin, Rule, RuleMatch, RuleSet, categorize_linear};
use tmon_core::{AppInfo, Categorizer, Direction, FlowRecord, RawFlow};

fn arb_match() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        (0u8..3).prop_map(|p| json!({"proto": [([1, 6, 17][p as usize])]})),
        (0u16..10).prop_map(|p| json!({"remote_port": [p]})),
        (0u16..10, 0u16..5).prop_map(|(a, w)| json!({"remote_port": [format!("{a}-{}", a + w)]})),
        (0u8..4).prop_map(|o| json!({"remote_cidr": [format!("10.{o}.0.0/16")]})),
        (0u32..4).prop_map(|a| json!({"asn": [a]})),
        (0u8..3).prop_map(|c| json!({"country": [(["DE", "US", "FR"][c as usize])]})),
        (0u8..2).prop_map(|h| json!({"hostname": [(["*.a.com", "x.b.org"][h as usize])]})),
        (0u8..4).prop_map(
            |d| json!({"direction": [(["up", "down", "internal", "transit"][d as usize])]})
        ),
        (0u16..10, 0u8..3)
            .prop_map(|(p, x)| json!({"remote_port": [p], "proto": [([1, 6, 17][x as usize])]})),
        (0u8..3).prop_map(|a| json!({"app": [(["BitTorrent", "steam", "WireGuard"][a as usize])]})),
        (0u8..2).prop_map(|c| json!({"app_category": [(["VPN", "game"][c as usize])]})),
        (0u8..3, 0u16..10).prop_map(
            |(a, p)| json!({"app": [(["bittorrent", "Steam", "x"][a as usize])], "remote_port": [p]})
        ),
    ]
}

fn arb_rules() -> impl Strategy<Value = Vec<Rule>> {
    prop::collection::vec((arb_match(), 0i64..50, 1u16..10, any::<bool>()), 0..25).prop_map(|v| {
        v.into_iter()
            .enumerate()
            .map(|(i, (m, prio, cat, enabled))| Rule {
                id: i as u32 + 1,
                category_id: cat,
                priority: prio,
                enabled,
                origin: Origin::Builtin,
                name: format!("r{i}"),
                match_: RuleMatch::from_json(&m).unwrap(),
            })
            .collect()
    })
}

fn arb_flow() -> impl Strategy<Value = FlowRecord> {
    (
        (0u8..4, 0u16..12, 0u8..3, 0u32..5, 0u8..4, 0u8..3, 0u8..4),
        0u8..5,
        any::<bool>(),
    )
        .prop_map(|((o, port, p, asn, cc, host, dir), app, confident)| {
            let raw = RawFlow {
                ts_ms: 0,
                ts_start_ms: 0,
                exporter_id: 1,
                in_if: 0,
                out_if: 0,
                src_ip: "192.168.0.1".parse().unwrap(),
                dst_ip: format!("10.{o}.1.1").parse().unwrap(),
                src_port: 1234,
                dst_port: port,
                proto: [1, 6, 17][p as usize],
                tcp_flags: 0,
                bytes: 1,
                packets: 1,
                sampled: false,
                nat_ip: None,
                nat_port: None,
                nat_dst_ip: None,
                nat_dst_port: None,
                flow_direction: None,
            };
            let hostname = ["www.a.com", "x.b.org", "c.net"][host as usize];
            let country = ["DE", "US", "FR", "JP"][cc as usize];
            let f = FlowRecord::from_raw(&raw, Direction::ALL[dir as usize], true)
                .with_geo(Some(country.into()), Some(asn), None)
                .with_remote_host(Some(hostname.into()));
            let apps = [
                ("BitTorrent", "Download"),
                ("Steam", "Game"),
                ("WireGuard", "VPN"),
            ];
            match apps.get(app as usize) {
                Some((a, c)) => f.with_app(&AppInfo::new(a, c, confident, None, false).unwrap()),
                None => f,
            }
        })
}

proptest! {
    #[test]
    fn indexed_equals_linear(rules in arb_rules(), flows in prop::collection::vec(arb_flow(), 1..20)) {
        let rs = RuleSet::compile(&rules, &HashMap::new());
        for f in &flows {
            let a = rs.categorize(f);
            prop_assert_eq!(a, categorize_linear(&rs, f));
            prop_assert_eq!(a, rs.categorize(f), "deterministic");
        }
    }
}
