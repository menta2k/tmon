//! Writes fuzz seeds for `dpi_proto_to_sidecar` and `dpi_proto_to_tmon`
//! under `backend/fuzz/corpus/<target>/`: one valid message of every type.

use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use tmon_dpi_proto::{
    CatalogEntry, Classification, Dir, FromClassifier, Hello, Key, Packet, Stats, ToClassifier,
    TopApp,
};

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus")
}

fn write(dir: &Path, name: &str, data: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(dir.join(name), data)
}

fn key(lan: &str, remote: &str, port: u16, proto: u8) -> Key {
    let ip = |s: &str| s.parse::<IpAddr>().expect("ip");
    Key {
        lan_ip: ip(lan),
        lan_port: 51001,
        remote_ip: ip(remote),
        remote_port: port,
        proto,
    }
}

/// A minimal IPv4/UDP packet (the classifier gets whole L3 packets).
fn ipv4_udp(payload: &[u8]) -> Vec<u8> {
    let total = (28 + payload.len()) as u16;
    let mut p = vec![0x45, 0, 0, 0, 0, 0, 0x40, 0, 64, 17, 0, 0];
    p[2..4].copy_from_slice(&total.to_be_bytes());
    p.extend_from_slice(&[192, 168, 88, 30, 198, 51, 100, 20]);
    p.extend_from_slice(&51001u16.to_be_bytes());
    p.extend_from_slice(&51820u16.to_be_bytes());
    p.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(payload);
    p
}

fn to_sidecar() -> Vec<(&'static str, Vec<u8>)> {
    let k4 = key("192.168.88.30", "198.51.100.20", 51820, 17);
    let k6 = key("fd00::10", "2001:db8::5", 443, 6);
    let mut wg = vec![1, 0, 0, 0, 0x11, 0x22, 0x33, 0x44];
    wg.resize(148, 0x5c);
    let packet = |key, dir, l3| {
        ToClassifier::Packet(Packet {
            key,
            dir,
            ts_ms: 1_767_225_600_000,
            l3,
        })
    };
    vec![
        (
            "packet_v4",
            packet(k4, Dir::LanToRemote, ipv4_udp(&wg)).encode(),
        ),
        (
            "packet_reply",
            packet(k4, Dir::RemoteToLan, ipv4_udp(&[2; 92])).encode(),
        ),
        ("forget_v6", ToClassifier::Forget(k6).encode()),
        ("ping", ToClassifier::Ping(42).encode()),
    ]
}

fn to_tmon() -> Vec<(&'static str, Vec<u8>)> {
    let entry = |a: &str, c: &str| CatalogEntry {
        app: a.into(),
        category: c.into(),
    };
    let hello = Hello {
        proto_version: 1,
        impl_name: "tmon-dpi".into(),
        impl_version: "0.3.0".into(),
        engine: "nDPI 6.0".into(),
        max_flows: 50_000,
        catalog: vec![entry("BitTorrent", "Download"), entry("WireGuard", "VPN")],
    };
    let mut out: Vec<(&'static str, Vec<u8>)> = hello
        .to_messages()
        .into_iter()
        .zip(["hello", "hello_catalog"])
        .map(|(m, n)| (n, m))
        .collect();
    let class = Classification {
        key: key("192.168.88.35", "198.51.100.25", 3389, 6),
        app: "RDP".into(),
        category: "RemoteAccess".into(),
        confident: true,
        final_: true,
        host: Some("desktop.example.net".into()),
    };
    let stats = Stats {
        packets_in: 1_000,
        flows_active: 12,
        classified_confident: 9,
        top: vec![TopApp {
            app: "BitTorrent".into(),
            flows: 4,
        }],
        ..Stats::default()
    };
    out.push((
        "classification",
        FromClassifier::Classification(class).encode(),
    ));
    out.push(("stats", FromClassifier::Stats(stats).encode()));
    out.push(("pong", FromClassifier::Pong(42).encode()));
    out
}

fn main() -> io::Result<()> {
    let root = corpus();
    for (name, bytes) in to_sidecar() {
        write(&root.join("dpi_proto_to_sidecar"), name, &bytes)?;
    }
    for (name, bytes) in to_tmon() {
        write(&root.join("dpi_proto_to_tmon"), name, &bytes)?;
    }
    println!("dpi fuzz seeds written to {}", root.display());
    Ok(())
}
