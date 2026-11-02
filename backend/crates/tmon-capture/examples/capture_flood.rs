//! Sends a capture mix to a tmon capture listener at a fixed rate (T066):
//! every connection has a fresh 5-tuple, so the tables fill to their caps.
//!   capture_flood <addr> <datagrams_per_sec> <secs>

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};
use tmon_capture::fixtures::hello::{Hello, client_hello, record};
use tmon_capture::fixtures::{scenarios, tcp_stream, udp};

fn tuple(i: u64) -> (SocketAddr, SocketAddr) {
    let c = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(
            192,
            168,
            (i >> 24) as u8 | 88,
            (i >> 16) as u8,
        )),
        (i as u16) | 1024,
    );
    let s = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(142, 250, (i >> 8) as u8, i as u8)),
        443,
    );
    (c, s)
}

fn usage() -> ! {
    eprintln!("usage: capture_flood <addr> <datagrams_per_sec> <secs>");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, to, rate, secs] = args.as_slice() else {
        usage()
    };
    let to: SocketAddr = to.parse().unwrap_or_else(|_| usage());
    let rate: u64 = rate.parse().unwrap_or_else(|_| usage());
    let secs: u64 = secs.parse().unwrap_or_else(|_| usage());
    let hello = record(&client_hello(&Hello {
        sni: Some("rr3.googlevideo.com"),
        alpn: &["h2"],
        ech: false,
        pq_key_share: true,
    }));
    let quic = scenarios::datagrams("quic_v1_rfc").remove(0);
    let quic = tmon_capture::frame::decode(&quic)
        .expect("fixture")
        .payload
        .to_vec();
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    let start = Instant::now();
    let (mut sent, mut conn) = (0u64, 0u64);
    while start.elapsed() < Duration::from_secs(secs) {
        conn += 1;
        let (c, s) = tuple(conn);
        // 3 TLS connections (SYN + 2 segments each) per QUIC Initial.
        let batch = if conn % 4 == 0 {
            vec![udp(c, s, &quic)]
        } else {
            tcp_stream(c, s, conn as u32, &hello, 1200)
        };
        for d in batch {
            let _ = sock.send_to(&d, to);
            sent += 1;
        }
        let due = Duration::from_secs_f64(sent as f64 / rate as f64);
        if let Some(wait) = due.checked_sub(start.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    println!(
        "sent {sent} capture datagrams ({conn} connections) in {:.1}s",
        start.elapsed().as_secs_f64()
    );
}
