//! Writes fuzz seed corpora under `backend/fuzz/corpus/<target>/` from the
//! capture scenarios (whole datagram sequences, ClientHellos, QUIC Initials,
//! DNS answers).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tmon_capture::fixtures::scenarios;

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus")
}

fn write(dir: &Path, name: &str, data: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(dir.join(name), data)
}

/// `capture_datagram` input format: `[len_hi, len_lo, bytes…]*`.
fn framed(datagrams: &[Vec<u8>]) -> Vec<u8> {
    datagrams
        .iter()
        .flat_map(|d| {
            let len = d.len().min(usize::from(u16::MAX)) as u16;
            len.to_be_bytes()
                .into_iter()
                .chain(d[..usize::from(len)].iter().copied())
        })
        .collect()
}

/// Application payload of a TZSP datagram (Ethernet + IPv4 + TCP/UDP).
fn payload(datagram: &[u8]) -> Option<Vec<u8>> {
    tmon_capture::frame::decode(datagram)
        .ok()
        .map(|p| p.payload.to_vec())
}

fn main() -> io::Result<()> {
    let root = corpus();
    for name in scenarios::ALL {
        let dgs = scenarios::datagrams(name);
        write(&root.join("capture_datagram"), name, &framed(&dgs))?;
        let target = match *name {
            n if n.starts_with("quic") => "quic_initial",
            n if n.starts_with("dns") => "dns_response",
            "fasttrack_syn_only" => continue,
            _ => "tls_client_hello",
        };
        // Joined TCP payloads give whole records; UDP scenarios give one seed per packet.
        let payloads: Vec<Vec<u8>> = dgs
            .iter()
            .filter_map(|d| payload(d))
            .filter(|p| !p.is_empty())
            .collect();
        if target == "tls_client_hello" {
            write(&root.join(target), name, &payloads.concat())?;
        } else {
            for (i, p) in payloads.iter().enumerate() {
                write(&root.join(target), &format!("{name}_{i}"), p)?;
            }
        }
    }
    println!("seeds written to {}", root.display());
    Ok(())
}
