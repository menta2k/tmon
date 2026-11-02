//! Built-in rule set categorization cost (target < 1 µs/flow, plan.md performance goals).
use std::collections::HashMap;
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tmon_core::category::Categorizer;
use tmon_core::direction::LocalNetworks;
use tmon_core::flow::{FlowRecord, RawFlow};
use tmon_core::rules::{RuleSet, load_builtin};

const PORTS: [u16; 10] = [443, 80, 53, 27015, 51820, 993, 123, 8443, 9999, 3478];

fn flows(n: usize) -> Vec<FlowRecord> {
    let nets = LocalNetworks::defaults();
    (0..n)
        .map(|i| {
            let port = PORTS[i % PORTS.len()];
            let raw = RawFlow {
                ts_ms: 1_767_225_600_000,
                ts_start_ms: 1_767_225_570_000,
                exporter_id: 1,
                in_if: 1,
                out_if: 2,
                src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 88, 2 + (i % 200) as u8)),
                dst_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, (i % 251) as u8)),
                src_port: 40_000 + (i % 20_000) as u16,
                dst_port: port,
                proto: if matches!(port, 53 | 123 | 27015 | 51820 | 3478) {
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
            };
            nets.to_record(&raw)
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let builtin = load_builtin().expect("builtin rules");
    let set = RuleSet::compile(&builtin.rules, &HashMap::new());
    let data = flows(1024);
    let mut g = c.benchmark_group("rules");
    g.throughput(Throughput::Elements(data.len() as u64));
    g.bench_function("categorize_builtin_1024", |b| {
        b.iter(|| {
            data.iter()
                .map(|f| set.categorize(black_box(f)).0 as u64)
                .sum::<u64>()
        })
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
