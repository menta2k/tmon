//! Capture pipeline with the classifier tap attached (T079, FR-018): the
//! feature 002 scenarios plus the application fixtures through one
//! `CaptureProcessor` that offers every packet to a running `DpiTap`.
//! Target: ≥ 50k datagrams/s, within a few percent of the tap-less run.

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use tmon_capture::Limits;
use tmon_capture::fixtures::{apps, scenarios};
use tmon_capture::health::Health;
use tmon_capture::pending::PendingRemoteNames;
use tmon_capture::processor::{CaptureProcessor, Outputs};
use tmon_capture::table::ConnTable;
use tmon_capture::tap::TapConfig;
use tmon_core::LocalNetworks;
use tmon_dpi_client::tap::DpiTap;

const ROUNDS: usize = 40;
const QUEUE: usize = 1 << 20;

fn mix() -> Vec<Vec<u8>> {
    let once: Vec<Vec<u8>> = scenarios::ALL
        .iter()
        .flat_map(|s| scenarios::datagrams(s))
        .chain(apps::mix())
        .collect();
    std::iter::repeat_n(once, ROUNDS).flatten().collect()
}

fn processor() -> CaptureProcessor {
    let limits = Limits::default();
    let health = Arc::new(Health::default());
    let out = Outputs {
        table: Arc::new(ConnTable::new(&limits, health.clone())),
        health,
        names: Arc::new(PendingRemoteNames::default()),
    };
    CaptureProcessor::new(out, limits)
}

/// A processor with a running tap; the receiver is returned so the queue
/// stays open (and is dropped with the batch).
fn tapped() -> (CaptureProcessor, tokio::sync::mpsc::Receiver<Vec<u8>>) {
    let (tap, rx) = DpiTap::new(QUEUE, usize::MAX, 32, 50_000);
    tap.set_running(true);
    let cfg = TapConfig::with_networks(Arc::new(tap), LocalNetworks::defaults());
    (processor().with_tap(cfg), rx)
}

fn run(p: &mut CaptureProcessor, datagrams: &[Vec<u8>]) {
    for (i, d) in datagrams.iter().enumerate() {
        p.handle(black_box(d), 1_700_000_000_000 + i as i64);
    }
}

fn bench(c: &mut Criterion) {
    let datagrams = mix();
    let mut g = c.benchmark_group("capture_tap");
    g.throughput(Throughput::Elements(datagrams.len() as u64));
    g.sample_size(20);
    g.bench_function("without_tap", |b| {
        b.iter_batched(
            processor,
            |mut p| run(&mut p, &datagrams),
            BatchSize::LargeInput,
        )
    });
    g.bench_function("with_dpi_tap", |b| {
        b.iter_batched(
            tapped,
            |(mut p, rx)| {
                run(&mut p, &datagrams);
                (p, rx)
            },
            BatchSize::LargeInput,
        )
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
