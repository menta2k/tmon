//! Fuzz crashers become regression tests (T064). Every file under
//! `backend/fuzz/artifacts/<target>/` and `tests/fuzz_regressions/<target>/`
//! is replayed through the same entry points as the fuzz target.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tmon_capture::health::Health;
use tmon_capture::pending::PendingRemoteNames;
use tmon_capture::processor::{CaptureProcessor, Outputs};
use tmon_capture::quic::{self, InitialKeys, crypto_frames};
use tmon_capture::table::ConnTable;
use tmon_capture::{Limits, dns, http, tls};

fn inputs(target: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    [
        root.join("../../fuzz/artifacts").join(target),
        root.join("tests/fuzz_regressions").join(target),
    ]
    .iter()
    .filter_map(|d| std::fs::read_dir(d).ok())
    .flatten()
    .filter_map(Result::ok)
    .map(|e| e.path())
    .filter(|p| p.is_file())
    .map(|p| {
        let data = std::fs::read(&p).unwrap();
        (p, data)
    })
    .collect()
}

fn capture_datagram(data: &[u8]) {
    let limits = Limits {
        max_connections: 64,
        max_reassembly_bytes_per_conn: 4096,
        max_reassembly_bytes_total: 16_384,
        max_device_bindings: 64,
        ..Limits::default()
    };
    let health = Arc::new(Health::default());
    let out = Outputs {
        table: Arc::new(ConnTable::new(&limits, health.clone())),
        health,
        names: Arc::new(PendingRemoteNames::default()),
    };
    let mut p = CaptureProcessor::new(out, limits);
    let (mut rest, mut now) = (data, 1_700_000_000_000i64);
    while rest.len() >= 2 {
        let len = (usize::from(rest[0]) << 8 | usize::from(rest[1])).min(rest.len() - 2);
        p.handle(&rest[2..2 + len], now);
        assert!(
            p.reassembly_bytes()
                <= limits.max_reassembly_bytes_total + limits.max_reassembly_bytes_per_conn
        );
        rest = &rest[2 + len..];
        now += 700;
    }
}

fn tls_client_hello(data: &[u8]) {
    let _ = tls::looks_like_client_hello(data);
    let _ = (tls::record_len(data), tls::handshake_len(data));
    let _ = tls::from_record(data);
    let _ = tls::from_handshake(data);
    let _ = http::host(data);
}

fn quic_initial(data: &[u8]) {
    for p in quic::packets(data).into_iter().flatten() {
        if let Ok(Some(v)) = p.initial_version()
            && let Ok(plain) = quic::decrypt(&p, &InitialKeys::client(v, p.header.dcid))
        {
            let _ = crypto_frames::crypto_chunks(&plain);
        }
    }
    let _ = crypto_frames::crypto_chunks(data);
}

fn dns_response(data: &[u8]) {
    for b in dns::bindings(data).unwrap_or_default() {
        assert!(b.aliases.len() <= tmon_core::naming::MAX_ALIASES);
        assert!((dns::MIN_TTL_SECS..=dns::MAX_TTL_SECS).contains(&b.ttl_secs));
    }
}

type Target = (&'static str, fn(&[u8]));

#[test]
fn replay_all_recorded_crashers() {
    let targets: [Target; 4] = [
        ("capture_datagram", capture_datagram),
        ("tls_client_hello", tls_client_hello),
        ("quic_initial", quic_initial),
        ("dns_response", dns_response),
    ];
    for (target, run) in targets {
        for (path, data) in inputs(target) {
            eprintln!("replaying {}", path.display());
            run(&data);
        }
    }
}

/// Hand-written edge cases kept even when the artifact directories are empty.
#[test]
fn edge_cases() {
    for d in [
        &[][..],
        &[0],
        &[0, 5, 1],
        &[0xff, 0xff],
        &[0, 4, 1, 0, 0, 1],
    ] {
        capture_datagram(d);
        tls_client_hello(d);
        quic_initial(d);
        dns_response(d);
    }
    // TLS record claiming 64 KiB; QUIC long header with a huge length varint.
    tls_client_hello(&[0x16, 0x03, 0x01, 0xff, 0xff, 0x01, 0xff, 0xff, 0xff]);
    quic_initial(&[
        0xc0, 0, 0, 0, 1, 8, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff,
    ]);
}
