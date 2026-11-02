//! Writes `sidecar_packets` fuzz seeds to `backend/dpi/fuzz/corpus/` from
//! the application fixtures (one framed message sequence per scenario).
//! Runs without nDPI: `cargo run -p tmon-dpi --no-default-features --example gen_fuzz_seeds`.

use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use tmon_capture::fixtures::apps::{self, APPS, AppExpect, PORT_ONLY, ZOOM};
use tmon_dpi_proto::{Dir, Key, Packet, ToClassifier};

fn message(e: &AppExpect, i: usize, d: &[u8]) -> Vec<u8> {
    let client: SocketAddr = e.client.parse().expect("client");
    let p = tmon_capture::frame::decode(d).expect("fixture decodes");
    let out = p.src_ip == client.ip() && p.src_port == client.port();
    let ((li, lp), (ri, rp)) = if out {
        ((p.src_ip, p.src_port), (p.dst_ip, p.dst_port))
    } else {
        ((p.dst_ip, p.dst_port), (p.src_ip, p.src_port))
    };
    ToClassifier::Packet(Packet {
        key: Key {
            lan_ip: li,
            lan_port: lp,
            remote_ip: ri,
            remote_port: rp,
            proto: p.proto(),
        },
        dir: if out {
            Dir::LanToRemote
        } else {
            Dir::RemoteToLan
        },
        ts_ms: 1_767_225_600_000 + i as u64,
        l3: p.l3.to_vec(),
    })
    .encode()
}

fn framed(e: &AppExpect) -> Vec<u8> {
    apps::datagrams(e)
        .iter()
        .enumerate()
        .flat_map(|(i, d)| {
            let m = message(e, i, d);
            (m.len() as u16).to_be_bytes().into_iter().chain(m)
        })
        .collect()
}

fn main() -> io::Result<()> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fuzz/corpus/sidecar_packets");
    fs::create_dir_all(&dir)?;
    for e in APPS.iter().chain([&ZOOM, &PORT_ONLY]) {
        fs::write(dir.join(e.scenario), framed(e))?;
    }
    println!("seeds written to {}", dir.display());
    Ok(())
}
