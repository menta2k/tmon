//! US2: top talkers and host drill-down (quickstart V6).

mod common;

use common::*;
use tmon_collector::listener::ListenerKind;

const RANGE: &str = "from=now-15m&to=now%2B1m";

async fn loaded() -> It {
    let it = start().await;
    let total: u64 = truth_by("mixed-home.truth.csv", 0).values().sum();
    it.replay("mixed-home.pcap", ListenerKind::Netflow).await;
    it.wait_for_bytes(&format!("/traffic/breakdown?by=category&{RANGE}"), total)
        .await;
    it
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn top_hosts_ranking_matches_truth() {
    let it = loaded().await;
    let truth = std::fs::read_to_string(fixture("mixed-home.truth.csv")).unwrap();
    let mut per_host: std::collections::BTreeMap<String, (u64, u64)> = Default::default();
    for l in truth.lines().skip(1) {
        let c: Vec<&str> = l.split(',').collect();
        let e = per_host.entry(c[2].to_string()).or_default();
        let b: u64 = c[3].parse().unwrap();
        if c[1] == "down" { e.0 += b } else { e.1 += b }
    }
    let mut expected: Vec<(&String, &(u64, u64))> = per_host.iter().collect();
    expected.sort_by_key(|e| std::cmp::Reverse(e.1.0 + e.1.1));

    let (status, body) = it.get(&format!("/top/hosts?{RANGE}&limit=10")).await;
    assert_eq!(status, 200, "{body}");
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), expected.len());
    for (row, (ip, (down, up))) in rows.iter().zip(expected) {
        assert_eq!(row["ip"], ip.as_str());
        assert!(
            within(row["bytes_down"].as_u64().unwrap(), *down, 1.0),
            "{ip} down"
        );
        assert!(
            within(row["bytes_up"].as_u64().unwrap(), *up, 1.0),
            "{ip} up"
        );
        assert!(row["top_category"].is_u64());
        assert!(row["flows"].as_u64().unwrap() > 0);
    }
    let (_, limited) = it.get(&format!("/top/hosts?{RANGE}&limit=2")).await;
    assert_eq!(limited["data"].as_array().unwrap().len(), 2);
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_detail_has_totals_splits_and_remotes() {
    let it = loaded().await;
    // Host rows are persisted by the tracker; flush it directly.
    it.app.state.hosts.flush().unwrap();
    let (status, body) = it.get(&format!("/hosts/192.168.88.10?{RANGE}")).await;
    assert_eq!(status, 200, "{body}");
    let d = &body["data"];
    assert_eq!(d["host"]["ip"], "192.168.88.10");
    assert!(d["totals"]["bytes_down"].as_u64().unwrap() > 0);
    assert!(!d["categories"].as_array().unwrap().is_empty());
    assert!(!d["protocols"].as_array().unwrap().is_empty());
    let remotes = d["top_remotes"].as_array().unwrap();
    assert!(!remotes.is_empty() && remotes.len() <= 20);

    let (missing, _) = it.get("/hosts/192.168.88.250").await;
    assert_eq!(missing, 404);
    let (bad, _) = it.get("/hosts/not-an-ip").await;
    assert_eq!(bad, 400);
    it.app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn top_dimensions_honor_limit_and_filters() {
    let it = loaded().await;
    for path in ["/top/remotes", "/top/asns", "/top/countries", "/top/ports"] {
        let (status, body) = it.get(&format!("{path}?{RANGE}&limit=3")).await;
        assert_eq!(status, 200, "{path}: {body}");
        let rows = body["data"].as_array().unwrap();
        assert!(
            !rows.is_empty() && rows.len() <= 3,
            "{path}: {}",
            rows.len()
        );
        assert!(body["meta"]["source"].is_string());
    }
    let (_, filtered) = it
        .get(&format!("/top/remotes?{RANGE}&host=192.168.88.12&limit=50"))
        .await;
    let all: u64 = filtered["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["bytes"].as_u64().unwrap())
        .sum();
    let truth = std::fs::read_to_string(fixture("mixed-home.truth.csv")).unwrap();
    let expected: u64 = truth
        .lines()
        .skip(1)
        .filter(|l| l.contains(",192.168.88.12,"))
        .map(|l| l.rsplit(',').next().unwrap().parse::<u64>().unwrap())
        .sum();
    assert!(
        within(all, expected, 1.0),
        "host filter: {all} vs {expected}"
    );

    let (_, ports) = it.get(&format!("/top/ports?{RANGE}&proto=17")).await;
    assert!(
        ports["data"].as_array().unwrap().iter().all(|r| r["key"]
            .as_str()
            .unwrap()
            .parse::<u16>()
            .is_ok())
    );
    for by in ["country", "asn"] {
        let (status, body) = it.get(&format!("/traffic/breakdown?by={by}&{RANGE}")).await;
        assert_eq!(status, 200, "{by}: {body}");
    }
    it.app.shutdown().await;
}
