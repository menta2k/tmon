//! Classify + geo + categorize throughput (target ≥ 100k flows/s/core).
//! Uses the MaxMind test databases in backend/tests/fixtures/geo when present.
use std::collections::HashMap;
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tmon_core::direction::LocalNetworks;
use tmon_core::flow::RawFlow;
use tmon_core::rules::{RuleSet, load_builtin};
use tmon_enrich::geoip::GeoIp;
use tmon_enrich::pipeline::{Pipeline, PipelineDeps};

const BATCH: usize = 4096;

fn raw(i: usize) -> RawFlow {
    let port = [443u16, 80, 53, 27015, 51820, 993, 123, 8443][i % 8];
    RawFlow {
        ts_ms: 1_767_225_600_000,
        ts_start_ms: 1_767_225_570_000,
        exporter_id: 1,
        in_if: 1,
        out_if: 2,
        src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 88, 2 + (i % 200) as u8)),
        // Spread remotes over many /16s so the geo LRU sees misses too.
        dst_ip: IpAddr::V4(Ipv4Addr::new(
            81 + (i % 120) as u8,
            (i / 7 % 256) as u8,
            (i / 3 % 256) as u8,
            1,
        )),
        src_port: 40_000 + (i % 20_000) as u16,
        dst_port: port,
        proto: if matches!(port, 53 | 123 | 27015 | 51820) {
            17
        } else {
            6
        },
        tcp_flags: 0x1b,
        bytes: 10_000,
        packets: 10,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    }
}

fn pipeline() -> Pipeline {
    let geo_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/geo");
    let deps = PipelineDeps {
        geo: GeoIp::open(
            &geo_dir.join("GeoIP2-Country-Test.mmdb"),
            &geo_dir.join("GeoLite2-ASN-Test.mmdb"),
        ),
        ..PipelineDeps::minimal()
    };
    let p = Pipeline::new(deps, LocalNetworks::defaults());
    let builtin = load_builtin().expect("builtin rules");
    p.set_rules(RuleSet::compile(&builtin.rules, &HashMap::new()));
    p
}

fn bench(c: &mut Criterion) {
    let p = pipeline();
    let batch: Vec<RawFlow> = (0..BATCH * 16).map(raw).collect();
    let mut g = c.benchmark_group("pipeline");
    g.throughput(Throughput::Elements(BATCH as u64));
    let mut off = 0;
    g.bench_function("enrich_batch_4096", |b| {
        b.iter(|| {
            off = (off + BATCH) % batch.len();
            black_box(p.process(&batch[off..off + BATCH]).len())
        })
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
