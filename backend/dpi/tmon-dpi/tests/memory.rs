//! Memory bounds with real nDPI (feature 003, T070 / SC-005).
#![cfg(feature = "ndpi")]

mod real;

use real::Sidecar;
use std::net::{IpAddr, Ipv4Addr};
use tmon_dpi_proto::{Dir, Key, Packet, ToClassifier};

/// Far more flows than fit in LIMIT_MB (an nDPI flow is a few KB).
const FLOWS: u32 = 50_000;
const LIMIT_MB: u64 = 32;

fn udp(i: u32) -> ToClassifier {
    let lan = Ipv4Addr::new(10, (i >> 16) as u8, (i >> 8) as u8, i as u8);
    let remote = Ipv4Addr::new(198, 51, 100, (i % 250) as u8 + 1);
    let mut l3 = vec![0x45, 0, 0, 0, 0, 0, 0x40, 0, 64, 17, 0, 0];
    l3.extend_from_slice(&lan.octets());
    l3.extend_from_slice(&remote.octets());
    l3.extend_from_slice(&((i % 60_000) as u16 + 1_024).to_be_bytes());
    l3.extend_from_slice(&40_000u16.to_be_bytes());
    let payload: Vec<u8> = (0..64)
        .map(|k| (i.wrapping_mul(2_654_435_761) >> (k % 24)) as u8)
        .collect();
    l3.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
    l3.extend_from_slice(&[0, 0]);
    l3.extend_from_slice(&payload);
    let total = l3.len() as u16;
    l3[2..4].copy_from_slice(&total.to_be_bytes());
    ToClassifier::Packet(Packet {
        key: Key {
            lan_ip: IpAddr::V4(lan),
            lan_port: (i % 60_000) as u16 + 1_024,
            remote_ip: IpAddr::V4(remote),
            remote_port: 40_000,
            proto: 17,
        },
        dir: Dir::LanToRemote,
        // Wall-clock time: the idle sweep compares against it.
        ts_ms: now_ms(),
        l3,
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn rss_bytes(pid: u32) -> u64 {
    let st = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    st.lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches(" kB").trim().parse::<u64>().ok())
        .map_or(0, |kb| kb * 1024)
}

#[test]
fn random_flows_stay_within_the_budget() {
    let mut s = Sidecar::spawn(&[
        "--memory-limit-mb",
        &LIMIT_MB.to_string(),
        "--max-flows",
        "1000000",
    ]);
    // nDPI's own tables are resident before any flow; the budget covers growth.
    let start_rss = rss_bytes(s.child.id());
    for i in 0..FLOWS {
        s.send(&udp(i));
        // Classifications come back while we send; read them so neither
        // side blocks on a full socket buffer (tmon reads concurrently).
        if i % 256 == 0 {
            s.drain();
        }
    }
    let stats = s.stats();
    let stats = if stats.packets_in < u64::from(FLOWS) {
        s.stats()
    } else {
        stats
    };
    assert!(s.child.try_wait().unwrap().is_none(), "sidecar died");
    assert!(
        stats.mem_bytes <= LIMIT_MB << 20,
        "{} live bytes",
        stats.mem_bytes
    );
    assert!(stats.evicted_memory > 0, "{stats:?}");
    let grown = rss_bytes(s.child.id()).saturating_sub(start_rss);
    assert!(grown < (LIMIT_MB + 64) << 20, "RSS grew by {grown}");
}
