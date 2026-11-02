//! Capture pipeline throughput (SC-004 / T065): a realistic mix of distinct
//! connections — split post-quantum TLS hellos, QUIC v1 Initials, plain HTTP,
//! DNS answers and SYN-only noise — through one `CaptureProcessor`.

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use hickory_proto::op::ResponseCode;
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tmon_capture::Limits;
use tmon_capture::fixtures::dns::{Answer, response};
use tmon_capture::fixtures::hello::{Hello, client_hello, record};
use tmon_capture::fixtures::{SYN, scenarios, tcp, tcp_stream, udp};
use tmon_capture::health::Health;
use tmon_capture::pending::PendingRemoteNames;
use tmon_capture::processor::{CaptureProcessor, Outputs};
use tmon_capture::table::ConnTable;

const CONNECTIONS: u32 = 4_000;

fn client(i: u32) -> SocketAddr {
    let ip = Ipv4Addr::new(192, 168, 88, (i % 200) as u8 + 10);
    SocketAddr::new(IpAddr::V4(ip), 20_000 + (i % 40_000) as u16)
}

fn server(i: u32) -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(142, 250, (i >> 8) as u8, i as u8)),
        443,
    )
}

fn mix() -> Vec<Vec<u8>> {
    let pq = record(&client_hello(&Hello {
        sni: Some("rr3.googlevideo.com"),
        alpn: &["h2"],
        ech: false,
        pq_key_share: true,
    }));
    let quic = scenarios::datagrams("quic_v1_rfc").remove(0);
    let quic_payload = tmon_capture::frame::decode(&quic).unwrap().payload.to_vec();
    let req = b"GET / HTTP/1.1\r\nHost: neverssl.com\r\n\r\n";
    let resolver: SocketAddr = scenarios::RESOLVER.parse().unwrap();
    (0..CONNECTIONS)
        .flat_map(|i| match i % 8 {
            0..=3 => tcp_stream(client(i), server(i), i * 7, &pq, 1400),
            4 | 5 => vec![udp(client(i), server(i), &quic_payload)],
            6 => {
                let ans = response(
                    "www.example.org",
                    &[Answer::Addr("www.example.org", server(i).ip(), 300)],
                    ResponseCode::NoError,
                );
                vec![udp(resolver, SocketAddr::new(client(i).ip(), 5353), &ans)]
            }
            _ => [tcp(client(i), server(i), i, SYN, &[])]
                .into_iter()
                .chain(
                    tcp_stream(client(i), server(i), i, req, 1400)
                        .into_iter()
                        .skip(1),
                )
                .collect(),
        })
        .collect()
}

fn processor_with_health() -> (CaptureProcessor, Arc<Health>) {
    let limits = Limits::default();
    let health = Arc::new(Health::default());
    let out = Outputs {
        table: Arc::new(ConnTable::new(&limits, health.clone())),
        health: health.clone(),
        names: Arc::new(PendingRemoteNames::default()),
    };
    (CaptureProcessor::new(out, limits), health)
}

fn processor() -> CaptureProcessor {
    processor_with_health().0
}

/// The mix must exercise the real work, not the `done` short-cut.
fn sanity_check(datagrams: &[Vec<u8>]) {
    let (mut p, h) = processor_with_health();
    for d in datagrams {
        p.handle(d, 1_700_000_000_000);
    }
    let c = h.counters.values();
    let per = u64::from(CONNECTIONS / 8);
    assert_eq!(c.tls_named, 4 * per, "{c:?}");
    assert_eq!(c.quic_named, 2 * per, "{c:?}");
    assert_eq!(c.http_named, per, "{c:?}");
    assert_eq!(c.dns_bindings, per, "{c:?}");
}

fn bench(c: &mut Criterion) {
    let datagrams = mix();
    sanity_check(&datagrams);
    let mut g = c.benchmark_group("capture");
    g.throughput(Throughput::Elements(datagrams.len() as u64));
    g.sample_size(20);
    g.bench_function("fixture_mix", |b| {
        b.iter_batched(
            processor,
            |mut p| {
                for (i, d) in datagrams.iter().enumerate() {
                    p.handle(black_box(d), 1_700_000_000_000 + i as i64);
                }
                p
            },
            BatchSize::LargeInput,
        )
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
