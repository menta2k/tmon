//! A scripted, pure-Rust stand-in for `tmon-dpi` (research R12). It speaks
//! protocol v1 on fd 3 and classifies by remote port, so supervisor,
//! back-pressure and end-to-end tests run without nDPI.
//!
//! Extra flags: `--mode classify|crash_after=N|hang_after=N|slow_us=N|garbage|silent`,
//! `--proto-version N`, `--stats-ms N`. tmon's own flags are accepted and ignored.

use std::collections::HashSet;
use std::os::fd::{FromRawFd, OwnedFd};
use std::time::{Duration, Instant};
use tmon_dpi_proto::*;
use tokio_seqpacket::UnixSeqpacket;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Classify,
    CrashAfter(u64),
    HangAfter(u64),
    SlowUs(u64),
    Garbage,
    Silent,
}

struct Args {
    mode: Mode,
    proto_version: u16,
    stats_ms: u64,
}

fn parse_mode(s: &str) -> Mode {
    let num = |p: &str| s.strip_prefix(p).and_then(|n| n.parse().ok()).unwrap_or(0);
    match s {
        "garbage" => Mode::Garbage,
        "silent" => Mode::Silent,
        _ if s.starts_with("crash_after=") => Mode::CrashAfter(num("crash_after=")),
        _ if s.starts_with("hang_after=") => Mode::HangAfter(num("hang_after=")),
        _ if s.starts_with("slow_us=") => Mode::SlowUs(num("slow_us=")),
        _ => Mode::Classify,
    }
}

fn parse_args() -> Args {
    let mut a = Args {
        mode: Mode::Classify,
        proto_version: PROTO_VERSION,
        stats_ms: 2_000,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    for pair in argv.windows(2) {
        match pair[0].as_str() {
            "--mode" => a.mode = parse_mode(&pair[1]),
            "--proto-version" => a.proto_version = pair[1].parse().unwrap_or(PROTO_VERSION),
            "--stats-ms" => a.stats_ms = pair[1].parse().unwrap_or(2_000),
            _ => {}
        }
    }
    a
}

/// What the fake reports for a remote port.
struct Verdict {
    app: &'static str,
    category: &'static str,
    confident: bool,
    final_: bool,
    host: Option<&'static str>,
}

const fn v(app: &'static str, category: &'static str, confident: bool, final_: bool) -> Verdict {
    Verdict {
        app,
        category,
        confident,
        final_,
        host: None,
    }
}

/// Verdict by remote port. 443 is never final, so packet budgets stay exact.
fn classify(port: u16) -> Option<Verdict> {
    Some(match port {
        6881..=6889 => v("BitTorrent", "Download", true, true),
        51820 => v("WireGuard", "VPN", false, false),
        27015 => v("Steam", "Game", true, true),
        3389 => Verdict {
            host: Some("desktop.example.net"),
            ..v("RDP", "RemoteAccess", true, true)
        },
        1194 => v("OpenVPN", "VPN", true, true),
        3478 => v("STUN", "Network", true, false),
        443 => Verdict {
            host: Some("fake.zoom.example"),
            ..v("Zoom", "Video", true, false)
        },
        _ => return None,
    })
}

fn hello(version: u16) -> Hello {
    let catalog = [
        "BitTorrent",
        "WireGuard",
        "Steam",
        "RDP",
        "OpenVPN",
        "STUN",
        "Zoom",
    ]
    .iter()
    .map(|a| CatalogEntry {
        app: (*a).into(),
        category: classify_cat(a).into(),
    })
    .collect();
    Hello {
        proto_version: version,
        impl_name: "fake_dpi".into(),
        impl_version: env!("CARGO_PKG_VERSION").into(),
        engine: "fake 1.0".into(),
        max_flows: 1_000,
        catalog,
    }
}

fn classify_cat(app: &str) -> &'static str {
    match app {
        "BitTorrent" => "Download",
        "WireGuard" | "OpenVPN" => "VPN",
        "Steam" => "Game",
        "RDP" => "RemoteAccess",
        "Zoom" => "Video",
        _ => "Network",
    }
}

struct State {
    packets: u64,
    seen: HashSet<Key>,
    confident: u64,
    guess: u64,
    started: Instant,
}

impl State {
    fn stats(&self) -> Stats {
        Stats {
            uptime_ms: self.started.elapsed().as_millis() as u64,
            packets_in: self.packets,
            flows_active: self.seen.len() as u32,
            flows_created: self.seen.len() as u64,
            classified_confident: self.confident,
            classified_guess: self.guess,
            mem_bytes: 1 << 20,
            mem_limit_bytes: 192 << 20,
            top: vec![TopApp {
                app: "BitTorrent".into(),
                flows: self.confident as u32,
            }],
            ..Stats::default()
        }
    }

    fn on_packet(&mut self, p: &Packet) -> Option<Vec<u8>> {
        self.packets += 1;
        let Verdict {
            app,
            category,
            confident,
            final_,
            host,
        } = classify(p.key.remote_port)?;
        if !self.seen.insert(p.key) {
            return None;
        }
        if confident {
            self.confident += 1;
        } else {
            self.guess += 1;
        }
        let c = Classification {
            key: p.key,
            app: app.into(),
            category: category.into(),
            confident,
            final_,
            host: host.map(String::from),
        };
        Some(FromClassifier::Classification(c).encode())
    }
}

async fn send(sock: &UnixSeqpacket, msg: &[u8]) {
    let _ = sock.send(msg).await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = parse_args();
    // SAFETY: tmon passes the classifier socket as fd 3 and nothing else
    // owns it in this process (contracts/dpi-ipc.md).
    let fd = unsafe { OwnedFd::from_raw_fd(3) };
    let sock = UnixSeqpacket::try_from(fd).expect("fd 3 is a seqpacket socket");
    if args.mode == Mode::Silent {
        tokio::time::sleep(Duration::from_secs(3_600)).await;
        return;
    }
    for m in hello(args.proto_version).to_messages() {
        send(&sock, &m).await;
    }
    if args.mode == Mode::Garbage {
        for i in 0..200u32 {
            send(&sock, &[0x82, (i % 251) as u8, 0xff, 0x00]).await;
        }
    }
    run(&sock, &args).await;
}

async fn run(sock: &UnixSeqpacket, args: &Args) {
    let mut st = State {
        packets: 0,
        seen: HashSet::new(),
        confident: 0,
        guess: 0,
        started: Instant::now(),
    };
    let mut tick = tokio::time::interval(Duration::from_millis(args.stats_ms.max(10)));
    let mut buf = vec![0u8; MAX_MESSAGE + 1];
    loop {
        tokio::select! {
            r = sock.recv(&mut buf) => {
                let n = match r { Ok(0) | Err(_) => return, Ok(n) => n };
                match ToClassifier::decode(&buf[..n]) {
                    Ok(ToClassifier::Packet(p)) => {
                        if let Some(m) = st.on_packet(&p) { send(sock, &m).await; }
                        after_packet(args.mode, st.packets).await;
                    }
                    Ok(ToClassifier::Ping(n)) => send(sock, &FromClassifier::Pong(n).encode()).await,
                    _ => {}
                }
            }
            _ = tick.tick() => send(sock, &FromClassifier::Stats(st.stats()).encode()).await,
        }
    }
}

async fn after_packet(mode: Mode, packets: u64) {
    match mode {
        Mode::CrashAfter(n) if packets >= n => std::process::exit(101),
        Mode::HangAfter(n) if packets >= n => loop {
            std::thread::sleep(Duration::from_secs(3_600));
        },
        Mode::SlowUs(us) => tokio::time::sleep(Duration::from_micros(us)).await,
        _ => {}
    }
}
