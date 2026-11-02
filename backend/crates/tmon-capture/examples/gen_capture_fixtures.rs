//! Writes the capture fixture pcaps to `backend/tests/fixtures/capture/`:
//! one TZSP pcap per scenario plus `capture_flows.pcap` (IPFIX flow records
//! for the named connections) and `capture_late_flow.pcap`.

use std::fs::{self, File};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tmon_capture::fixtures::scenarios::{self, Expect, LATE};
use tmon_collector::fixtures::corpus::{BASE_MS, flow, ipfix_packets};
use tmon_collector::pcap::{UdpPacket, write_udp};

const ROUTER: &str = "192.168.88.1:37008";
const CAPTURE_DST: &str = "127.0.0.1:37008";

fn out_dir() -> PathBuf {
    std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/capture")
        })
}

fn capture_pcap(dir: &Path, name: &str) -> io::Result<usize> {
    let src: SocketAddr = ROUTER.parse().expect("addr");
    let dst: SocketAddr = CAPTURE_DST.parse().expect("addr");
    let pkts: Vec<UdpPacket> = scenarios::datagrams(name)
        .into_iter()
        .enumerate()
        .map(|(i, payload)| UdpPacket {
            ts_us: (BASE_MS + i as i64) * 1000,
            src,
            dst,
            payload,
        })
        .collect();
    write_udp(File::create(dir.join(format!("{name}.pcap")))?, &pkts)?;
    Ok(pkts.len())
}

fn flows(expect: &[Expect]) -> Vec<UdpPacket> {
    let specs: Vec<_> = expect
        .iter()
        .map(|e| {
            let (c, s): (SocketAddr, SocketAddr) =
                (e.client.parse().expect("c"), e.server.parse().expect("s"));
            flow(
                &c.ip().to_string(),
                &s.ip().to_string(),
                c.port(),
                s.port(),
                e.proto,
                250_000,
            )
        })
        .collect();
    ipfix_packets(&specs)
}

fn main() -> io::Result<()> {
    let dir = out_dir();
    fs::create_dir_all(&dir)?;
    for name in scenarios::ALL {
        let n = capture_pcap(&dir, name)?;
        println!("{name}.pcap: {n} datagrams");
    }
    write_udp(
        File::create(dir.join("capture_flows.pcap"))?,
        &flows(scenarios::EXPECTED),
    )?;
    write_udp(
        File::create(dir.join("capture_late_flow.pcap"))?,
        &flows(&[LATE]),
    )?;
    println!(
        "capture_flows.pcap, capture_late_flow.pcap written to {}",
        dir.display()
    );
    Ok(())
}
