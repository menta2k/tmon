//! Classifier throughput with the real nDPI (T079): the application fixture
//! mix, replayed as fresh connections, through `Flows<ndpi::Module>` on one
//! core. Target: ≥ 20k packets/s. Run with `just dpi-build` first:
//!   cargo bench --manifest-path backend/dpi/Cargo.toml -p tmon-dpi --bench classify

#[cfg(feature = "ndpi")]
fn main() {
    bench::run();
}

#[cfg(not(feature = "ndpi"))]
fn main() {
    eprintln!("classify bench needs the `ndpi` feature (just dpi-build)");
}

#[cfg(feature = "ndpi")]
mod bench {
    use std::hint::black_box;
    use std::time::Instant;
    use tmon_capture::fixtures::apps::{self, AppExpect};
    use tmon_dpi::flows::{Flows, Limits};
    use tmon_dpi::ndpi::Module;
    use tmon_dpi_proto::{Dir, Key, Packet};

    const ROUNDS: u16 = 2_000;
    const TARGET_PPS: f64 = 20_000.0;

    /// One scenario's packets, oriented on its client.
    fn packets(e: &AppExpect) -> Vec<Packet> {
        let client: std::net::SocketAddr = e.client.parse().expect("fixture client");
        apps::datagrams(e)
            .iter()
            .map(|d| {
                let p = tmon_capture::frame::decode(d).expect("fixture decodes");
                let out = p.src_ip == client.ip() && p.src_port == client.port();
                let (lan, remote) = if out {
                    ((p.src_ip, p.src_port), (p.dst_ip, p.dst_port))
                } else {
                    ((p.dst_ip, p.dst_port), (p.src_ip, p.src_port))
                };
                Packet {
                    key: Key {
                        lan_ip: lan.0,
                        lan_port: lan.1,
                        remote_ip: remote.0,
                        remote_port: remote.1,
                        proto: p.proto(),
                    },
                    dir: if out {
                        Dir::LanToRemote
                    } else {
                        Dir::RemoteToLan
                    },
                    ts_ms: 0,
                    l3: p.l3.to_vec(),
                }
            })
            .collect()
    }

    /// The mix, with each round on new LAN ports so every round is a new flow.
    fn workload() -> Vec<Packet> {
        let base: Vec<Packet> = apps::APPS
            .iter()
            .chain([&apps::ZOOM, &apps::PORT_ONLY])
            .flat_map(packets)
            .collect();
        (0..ROUNDS)
            .flat_map(|r| {
                base.iter().map(move |p| Packet {
                    key: Key {
                        lan_port: p.key.lan_port.wrapping_add(r),
                        ..p.key
                    },
                    ts_ms: u64::from(r),
                    ..p.clone()
                })
            })
            .collect()
    }

    pub fn run() {
        let work = workload();
        let module = Module::new().expect("nDPI init");
        let limits = Limits {
            max_flows: 50_000,
            max_packets: 32,
            idle_timeout_ms: 900_000,
            mem_limit_bytes: 192 << 20,
        };
        let mut flows = Flows::new(module, limits);
        let start = Instant::now();
        let classified = work
            .iter()
            .filter(|p| black_box(flows.packet(p)).is_some())
            .count();
        let secs = start.elapsed().as_secs_f64();
        let pps = work.len() as f64 / secs;
        println!(
            "classify: {} packets, {} classifications in {secs:.3}s → {pps:.0} packets/s (target ≥ {TARGET_PPS})",
            work.len(),
            classified
        );
        assert!(pps >= TARGET_PPS, "below target");
    }
}
