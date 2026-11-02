//! Load generator for `scripts/bench-ingest.sh`:
//! `cargo run --release -p tmon-collector --example flood -- <addr> <flows_per_sec> <seconds>`.
//! Sends NetFlow v5 datagrams (30 flows each) with current timestamps, paced every 10 ms,
//! over 250 local hosts × 4096 remotes × varied ports so categorization and rollups do real work.
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tmon_collector::fixtures::encode::{FlowSpec, encode_v5};

const PER_DATAGRAM: usize = 30;
const TICK: Duration = Duration::from_millis(10);
const PORTS: [u16; 8] = [443, 80, 53, 27015, 51820, 993, 123, 8080];

fn arg<T: std::str::FromStr>(i: usize, default: T) -> T {
    std::env::args()
        .nth(i)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn spec(n: u64, now: i64) -> FlowSpec {
    let host = Ipv4Addr::new(192, 168, 88, 2 + (n % 250) as u8);
    let r = (n / 250) % 4096;
    let remote = Ipv4Addr::new(203, 0, 112 + (r / 256) as u8 % 2, (r % 256) as u8);
    let port = PORTS[(n % PORTS.len() as u64) as usize];
    let bytes = 1_000 + (n * 7919) % 2_000_000;
    FlowSpec {
        start_ms: now - 30_000,
        end_ms: now - 500,
        src: IpAddr::V4(host),
        dst: IpAddr::V4(remote),
        sport: 40_000 + (n % 20_000) as u16,
        dport: port,
        proto: if port == 53 || port == 123 || port == 51820 || port == 27015 {
            17
        } else {
            6
        },
        tcp_flags: 0x1b,
        bytes,
        packets: bytes.div_ceil(1200),
        in_if: 1,
        out_if: 2,
        nat: None,
        nat_dst: None,
    }
}

fn main() -> std::io::Result<()> {
    let addr: String = arg(1, "127.0.0.1:2055".to_string());
    let rate: u64 = arg(2, 10_000);
    let secs: u64 = arg(3, 600);
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    let per_tick = (rate * TICK.as_millis() as u64 / 1000).max(1);
    let (start, mut n, mut seq) = (Instant::now(), 0u64, 0u32);
    while start.elapsed() < Duration::from_secs(secs) {
        let tick_start = Instant::now();
        let now = now_ms();
        let flows: Vec<FlowSpec> = (0..per_tick).map(|i| spec(n + i, now)).collect();
        for chunk in flows.chunks(PER_DATAGRAM) {
            sock.send_to(&encode_v5(now, seq, 0, chunk), &addr)?;
            seq = seq.wrapping_add(chunk.len() as u32);
        }
        n += per_tick;
        if let Some(rest) = TICK.checked_sub(tick_start.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    let el = start.elapsed().as_secs_f64();
    println!("sent {n} flows in {el:.1}s ({:.0} flows/s)", n as f64 / el);
    Ok(())
}
