//! Enrichment pipeline: RawFlow → classified, enriched, categorized FlowRecord.

use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use tmon_collector::{CountDirection, DropReason, ExporterRegistry, GlobalCounters};
use tmon_core::app::{AppInfo, ConnectionClassifier};
use tmon_core::naming::{ConnKey, ConnName, ConnectionNamer, NameSource};
use tmon_core::rules::RuleSet;
use tmon_core::{Categorizer, FlowRecord, LocalNetworks, RawFlow};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::geo::{GeoLookup, NoGeo, is_private};

/// Remote IP → hostname (from the router DNS cache).
pub type RemoteNames = HashMap<IpAddr, Arc<str>>;

/// Hook receiving every enriched batch (live aggregation, host tracking…).
pub type BatchHook = Arc<dyn Fn(&[FlowRecord]) + Send + Sync>;

/// Everything the pipeline needs; swappable parts are behind `ArcSwap`.
pub struct PipelineDeps {
    pub registry: Arc<ExporterRegistry>,
    pub globals: Arc<GlobalCounters>,
    pub geo: Arc<dyn GeoLookup>,
}

impl PipelineDeps {
    pub fn minimal() -> Self {
        Self {
            registry: Arc::new(ExporterRegistry::new()),
            globals: Arc::new(GlobalCounters::default()),
            geo: Arc::new(NoGeo),
        }
    }
}

pub struct Pipeline {
    deps: PipelineDeps,
    networks: ArcSwap<LocalNetworks>,
    rules: ArcSwap<RuleSet>,
    remote_names: ArcSwap<RemoteNames>,
    hooks: ArcSwap<Vec<BatchHook>>,
    /// Connection naming step (FR-013), e.g. packet capture. Set once at startup.
    namer: OnceLock<Arc<dyn ConnectionNamer>>,
    /// Application classification step (feature 003). Set once at startup.
    classifier: OnceLock<Arc<dyn ConnectionClassifier>>,
}

impl Pipeline {
    pub fn new(deps: PipelineDeps, networks: LocalNetworks) -> Self {
        Self {
            deps,
            networks: ArcSwap::from_pointee(networks),
            rules: ArcSwap::from_pointee(RuleSet::default()),
            remote_names: ArcSwap::from_pointee(RemoteNames::new()),
            hooks: ArcSwap::from_pointee(Vec::new()),
            namer: OnceLock::new(),
            classifier: OnceLock::new(),
        }
    }

    pub fn set_networks(&self, n: LocalNetworks) {
        self.networks.store(Arc::new(n));
    }

    pub fn networks(&self) -> Arc<LocalNetworks> {
        self.networks.load_full()
    }

    pub fn set_rules(&self, r: RuleSet) {
        self.rules.store(Arc::new(r));
    }

    pub fn rules(&self) -> Arc<RuleSet> {
        self.rules.load_full()
    }

    pub fn set_remote_names(&self, names: RemoteNames) {
        self.remote_names.store(Arc::new(names));
    }

    pub fn remote_names(&self) -> Arc<RemoteNames> {
        self.remote_names.load_full()
    }

    pub fn add_hook(&self, hook: BatchHook) {
        let mut next = (*self.hooks.load_full()).clone();
        next.push(hook);
        self.hooks.store(Arc::new(next));
    }

    /// Install the naming step; returns false if one is already set.
    pub fn set_namer(&self, namer: Arc<dyn ConnectionNamer>) -> bool {
        self.namer.set(namer).is_ok()
    }

    /// Install the classification step; returns false if one is already set.
    pub fn set_classifier(&self, c: Arc<dyn ConnectionClassifier>) -> bool {
        self.classifier.set(c).is_ok()
    }

    pub fn deps(&self) -> &PipelineDeps {
        &self.deps
    }

    /// Enrich one flow (no exporter filtering).
    pub fn enrich(&self, raw: &RawFlow) -> FlowRecord {
        let rec = self.networks.load().to_record(raw);
        let rec = if is_private(&rec.remote_ip) {
            rec
        } else {
            let g = self.deps.geo.lookup(rec.remote_ip);
            rec.with_geo(g.country, g.asn, g.as_name)
        };
        let rec = self.name(rec);
        let rec = self.classify(rec);
        let (cat, rule) = self.rules.load().categorize(&rec);
        rec.with_category(cat, rule)
    }

    /// Remote name in FR-010 order: this connection's setup name, the
    /// device's DNS binding, the router DNS cache, then an ECH front name.
    fn name(&self, rec: FlowRecord) -> FlowRecord {
        let found = self
            .namer
            .get()
            // Host clock, like the capture side: the router's flow timestamps
            // may be skewed and must not stretch or shorten name TTLs.
            .map(|n| n.lookup(&ConnKey::from_record(&rec), host_now_ms()))
            .unwrap_or_default();
        let (setup, ech) = match found.connection {
            Some(c) if c.ech_outer => (None, Some(c)),
            other => (other, None),
        };
        if let Some(n) = setup.or(found.device) {
            return apply(&rec, &n);
        }
        if let Some(name) = self.remote_names.load().get(&rec.remote_ip) {
            return rec.with_remote_name(Arc::clone(name), NameSource::Router);
        }
        ech.map_or(rec.clone(), |n| apply(&rec, &n))
    }

    /// Application step (FR-004/FR-005): after naming, so a feature 002 name
    /// always wins; the classifier's host is used only when nothing else
    /// named the flow.
    fn classify(&self, rec: FlowRecord) -> FlowRecord {
        let Some(c) = self.classifier.get() else {
            return rec;
        };
        match c.classify(&ConnKey::from_record(&rec), host_now_ms()) {
            Some(info) => apply_app(&rec, &info),
            None => rec,
        }
    }

    /// Enrich a batch, dropping flows excluded by the exporter's count direction.
    pub fn process(&self, batch: &[RawFlow]) -> Vec<FlowRecord> {
        let mut out = Vec::with_capacity(batch.len());
        let mut dirs: HashMap<u16, CountDirection> = HashMap::new();
        for raw in batch {
            let cd = *dirs.entry(raw.exporter_id).or_insert_with(|| {
                self.deps
                    .registry
                    .get_by_id(raw.exporter_id)
                    .map(|e| e.settings().count_direction)
                    .unwrap_or_default()
            });
            if excluded(cd, raw.flow_direction) {
                self.deps.globals.drop_n(DropReason::DirectionFilter, 1);
                continue;
            }
            out.push(self.enrich(raw));
        }
        for hook in self.hooks.load().iter() {
            hook(&out);
        }
        out
    }
}

fn apply(rec: &FlowRecord, n: &ConnName) -> FlowRecord {
    rec.with_remote_name(Arc::clone(&n.host), n.source)
        .with_aliases(Arc::clone(&n.aliases))
}

fn apply_app(rec: &FlowRecord, info: &AppInfo) -> FlowRecord {
    let rec = rec.with_app(info);
    match &info.host {
        Some(h) if rec.remote_host.is_none() => {
            rec.with_remote_name(Arc::clone(h), NameSource::Dpi)
        }
        _ => rec,
    }
}

/// `ingress` keeps only ingress-reported records (and vice versa).
fn excluded(cd: CountDirection, reported: Option<u8>) -> bool {
    matches!(
        (cd, reported),
        (CountDirection::Ingress, Some(1)) | (CountDirection::Egress, Some(0))
    )
}

/// Destination for enriched batches (the storage writer in production).
pub trait BatchSink: Send + Sync + 'static {
    fn send(&self, flows: Vec<FlowRecord>) -> impl std::future::Future<Output = bool> + Send;
}

impl BatchSink for tmon_store::duck::writer::WriterHandle {
    async fn send(&self, flows: Vec<FlowRecord>) -> bool {
        tmon_store::duck::writer::WriterHandle::send(self, flows)
            .await
            .is_ok()
    }
}

/// Run the pipeline worker until the collector channel closes.
pub fn spawn<S: BatchSink>(
    pipeline: Arc<Pipeline>,
    mut rx: mpsc::Receiver<Vec<RawFlow>>,
    sink: S,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(batch) = rx.recv().await {
            let flows = pipeline.process(&batch);
            if !flows.is_empty() && !sink.send(flows).await {
                tracing::error!("storage writer stopped; pipeline exiting");
                break;
            }
        }
    })
}

fn host_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
