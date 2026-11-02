//! Application wiring: collectors → pipeline → store, plus the HTTP server.

use anyhow::Context;
use arc_swap::ArcSwap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tmon_api::AppState;
use tmon_api::auth::Auth;
use tmon_api::routes::settings::policy_of;
use tmon_collector::exporter_registry::STALL_AFTER_MS;
use tmon_collector::health::HealthMonitor;
use tmon_collector::listener::{self, ListenerCtx, ListenerInfo, ListenerKind};
use tmon_collector::{ExporterRegistry, GlobalCounters};
use tmon_enrich::geoip::GeoIp;
use tmon_enrich::hosts::HostTracker;
use tmon_enrich::router_poll;
use tmon_enrich::{LiveTap, Pipeline, PipelineDeps, ruleset_handle};
use tmon_store::duck::query::router::Windows;
use tmon_store::duck::repo::settings::{
    self, Retention, Router, Settings, Storage as SettingsStorage,
};
use tmon_store::duck::repo::{exporters, networks};
use tmon_store::duck::writer::{self, WriterHandle};
use tmon_store::{DuckStore, StoreOptions};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::Validated;

/// Records per batch on average; converts `queue_capacity` (flows) to batches.
const FLOWS_PER_BATCH: usize = 32;

/// A running instance (used by `serve` and integration tests).
pub struct Running {
    pub http_addr: SocketAddr,
    pub state: AppState,
    shutdown: watch::Sender<bool>,
    writer: Option<WriterHandle>,
    writer_task: JoinHandle<()>,
    tasks: Vec<JoinHandle<()>>,
}

impl Running {
    /// Bound UDP address of the first listener of a kind.
    pub fn udp_addr(&self, kind: ListenerKind) -> Option<SocketAddr> {
        self.state
            .listeners
            .load()
            .iter()
            .find(|l| l.proto == kind && l.listening)
            .and_then(|l| l.address.parse().ok())
    }

    /// Flush buffered flows to storage.
    pub async fn flush(&self) -> anyhow::Result<()> {
        match &self.writer {
            Some(w) => Ok(w.flush().await?),
            None => Ok(()),
        }
    }

    /// Stop listeners, drain the pipeline and flush storage.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown.send(true);
        self.writer = None;
        for t in self.tasks.drain(..) {
            t.abort();
        }
        let _ = tokio::time::timeout(Duration::from_secs(30), self.writer_task).await;
    }
}

/// Stall threshold; `TMON_TEST_STALL_SECS` shortens it for tests (quickstart V10).
fn stall_after_ms() -> i64 {
    std::env::var("TMON_TEST_STALL_SECS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .map_or(STALL_AFTER_MS, |n| n * 1000)
}

/// Settings from the config file/env, used until the UI saves its own.
pub fn file_settings(v: &Validated) -> Settings {
    let (r, rt) = (&v.config.retention, &v.config.router);
    Settings {
        retention: Retention {
            raw_days: r.raw_days,
            rollup_1m_days: r.rollup_1m_days,
            rollup_1h_days: r.rollup_1h_days,
        },
        storage: SettingsStorage {
            disk_cap_gb: v.config.storage.disk_cap_gb,
        },
        router: Router {
            kind: rt.kind,
            enabled: rt.enabled,
            url: rt.url.clone(),
            username: rt.username.clone(),
            password: rt.password.clone(),
            verify_tls: rt.verify_tls,
            lease_poll_secs: rt.lease_poll_secs,
            dns_poll_secs: rt.dns_poll_secs,
        },
    }
}

async fn load_settings(v: &Validated, store: &Arc<DuckStore>) -> anyhow::Result<Settings> {
    let (s, defaults) = (Arc::clone(store), file_settings(v));
    Ok(tokio::task::spawn_blocking(move || settings::effective(&s.conn()?, &defaults)).await??)
}

async fn open_store(v: &Validated) -> anyhow::Result<Arc<DuckStore>> {
    let dir = v.config.data_dir.clone();
    let opts = StoreOptions {
        memory_limit: v.config.storage.memory_limit.clone(),
        threads: v.config.storage.threads,
    };
    let store = tokio::task::spawn_blocking(move || DuckStore::open(&dir, &opts))
        .await?
        .context("opening storage")?;
    Ok(Arc::new(store))
}

async fn build_pipeline(
    v: &Validated,
    store: &Arc<DuckStore>,
    registry: Arc<ExporterRegistry>,
    globals: Arc<GlobalCounters>,
) -> anyhow::Result<(Arc<Pipeline>, Arc<GeoIp>)> {
    let s = Arc::clone(store);
    let nets = tokio::task::spawn_blocking(move || networks::load(&s.conn()?)).await??;
    let (c, a) = (
        v.config.geoip.country_db.clone(),
        v.config.geoip.asn_db.clone(),
    );
    let geo = tokio::task::spawn_blocking(move || GeoIp::open(&c, &a)).await?;
    let deps = PipelineDeps {
        registry,
        globals,
        geo: geo.clone(),
    };
    let pipeline = Arc::new(Pipeline::new(deps, nets));
    let (s, p) = (Arc::clone(store), Arc::clone(&pipeline));
    let n = tokio::task::spawn_blocking(move || ruleset_handle::init(&s, &p))
        .await?
        .context("loading rules")?;
    tracing::info!(rules = n, "categorization rules loaded");
    Ok((pipeline, geo))
}

fn bind_all(
    v: &Validated,
    ctx: &ListenerCtx,
    stop: &watch::Receiver<bool>,
) -> (Vec<ListenerInfo>, Vec<JoinHandle<()>>) {
    let want = v
        .netflow_listen
        .iter()
        .map(|a| (*a, ListenerKind::Netflow))
        .chain(v.sflow_listen.iter().map(|a| (*a, ListenerKind::Sflow)));
    let mut infos = Vec::new();
    let mut tasks = Vec::new();
    for (addr, kind) in want {
        match listener::bind(addr, v.config.collector.socket_recv_buffer_bytes) {
            Ok(sock) => {
                let local = sock.local_addr().unwrap_or(addr);
                tracing::info!(%local, ?kind, "collector listening");
                infos.push(ListenerInfo {
                    proto: kind,
                    port: local.port(),
                    address: local.to_string(),
                    listening: true,
                    error: None,
                });
                tasks.push(listener::spawn(sock, kind, ctx.clone(), stop.clone()));
            }
            Err(e) => {
                tracing::error!(%addr, error = %e, "cannot bind collector");
                infos.push(ListenerInfo {
                    proto: kind,
                    port: addr.port(),
                    address: addr.to_string(),
                    listening: false,
                    error: Some(e.to_string()),
                });
            }
        }
    }
    (infos, tasks)
}

/// Start everything; returns once all sockets are bound.
pub async fn start(v: Validated) -> anyhow::Result<Running> {
    let store = open_store(&v).await?;
    let s = Arc::clone(&store);
    let known = tokio::task::spawn_blocking(move || exporters::list(&s.conn()?)).await??;
    let registry = Arc::new(ExporterRegistry::with_known(crate::persist::known(known)));
    let globals = Arc::new(GlobalCounters::default());
    let (pipeline, geo) =
        build_pipeline(&v, &store, Arc::clone(&registry), Arc::clone(&globals)).await?;
    let flush = Duration::from_secs(v.config.storage.flush_interval_secs);
    let (writer, writer_task) = writer::spawn(Arc::clone(&store), flush);
    let (tx, rx) = mpsc::channel((v.config.collector.queue_capacity / FLOWS_PER_BATCH).max(16));
    let (exp_tx, exp_rx) = mpsc::unbounded_channel();
    let ctx = ListenerCtx {
        registry: Arc::clone(&registry),
        globals: Arc::clone(&globals),
        tx,
        allowed: Arc::new(v.allowed_exporters.clone()),
        new_exporters: Some(exp_tx),
    };
    let (stop_tx, stop_rx) = watch::channel(false);
    let live = LiveTap::new();
    pipeline.add_hook(live.hook());
    let hosts = HostTracker::new(Arc::clone(&store));
    pipeline.add_hook(hosts.hook());
    let clock: tmon_api::state::Clock = Arc::new(tmon_store::duck::now_ms);
    let (infos, mut tasks) = bind_all(&v, &ctx, &stop_rx);
    drop(ctx);
    let inputs = crate::dpi::start_inputs(&v, &pipeline, &store, &clock, &stop_rx);
    tasks.extend(inputs.tasks);
    tasks.push(tmon_enrich::pipeline::spawn(
        Arc::clone(&pipeline),
        rx,
        writer.clone(),
    ));
    tasks.push(hosts.spawn());
    let health = HealthMonitor::new(Arc::clone(&registry));
    tasks.push(health.spawn());
    let effective = load_settings(&v, &store).await?;
    let settings_swap = Arc::new(ArcSwap::from_pointee(effective));
    let router_status = Arc::new(ArcSwap::from_pointee(
        tmon_enrich::routeros::RouterStatus::default(),
    ));
    let s = Arc::clone(&store);
    pipeline.set_remote_names(
        tokio::task::spawn_blocking(move || router_poll::load_remote_names(&s)).await??,
    );
    tasks.push(router_poll::spawn(router_poll::RouterDeps {
        store: Arc::clone(&store),
        pipeline: Arc::clone(&pipeline),
        hosts: Arc::clone(&hosts),
        settings: Arc::clone(&settings_swap),
        status: Arc::clone(&router_status),
        clock: Arc::clone(&clock),
    }));
    let pol = policy_of(&settings_swap.load());
    let policy = Arc::new(ArcSwap::from_pointee(pol));
    let external_warnings = Arc::new(ArcSwap::from_pointee(Vec::new()));
    tasks.push(crate::janitor::spawn(
        Arc::clone(&store),
        Arc::clone(&policy),
        Arc::clone(&external_warnings),
    ));
    tasks.push(geo.spawn_watcher());
    tasks.push(live.spawn(tmon_enrich::live_tap::TICK, Arc::clone(&clock)));
    tasks.push(crate::persist::spawn(
        Arc::clone(&store),
        Arc::clone(&registry),
        exp_rx,
    ));
    let auth = Auth::new(
        v.config.http.password_hash.clone(),
        Some(v.config.http.api_token.clone()).filter(|t| !t.trim().is_empty()),
        v.config.http.session_ttl_hours,
    );
    let state = AppState {
        store,
        registry,
        globals,
        pipeline,
        auth: Arc::new(auth),
        listeners: Arc::new(ArcSwap::from_pointee(infos)),
        windows: Arc::new(ArcSwap::from_pointee(Windows {
            raw_ms: pol.raw_ms,
            rollup_1m_ms: pol.rollup_1m_ms,
        })),
        clock,
        live,
        hosts,
        stall_after_ms: stall_after_ms(),
        external_warnings,
        health,
        policy,
        router: router_status.clone(),
        jobs: tmon_api::jobs::Jobs::spawn(),
        settings: settings_swap.clone(),
        hits_cache: Default::default(),
        capture: inputs.capture,
        dpi: inputs.dpi,
    };
    let (http_addr, server) = serve_http(v.http_listen, state.clone(), stop_rx).await?;
    tasks.push(server);
    Ok(Running {
        http_addr,
        state,
        shutdown: stop_tx,
        writer: Some(writer),
        writer_task,
        tasks,
    })
}

async fn serve_http(
    addr: SocketAddr,
    state: AppState,
    mut stop: watch::Receiver<bool>,
) -> anyhow::Result<(SocketAddr, JoinHandle<()>)> {
    let tcp = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding HTTP {addr}"))?;
    let local = tcp.local_addr()?;
    let app = tmon_api::router(state).into_make_service_with_connect_info::<SocketAddr>();
    let task = tokio::spawn(async move {
        let shutdown = async move {
            let _ = stop.wait_for(|v| *v).await;
        };
        if let Err(e) = axum::serve(tcp, app).with_graceful_shutdown(shutdown).await {
            tracing::error!(error = %e, "http server failed");
        }
    });
    tracing::info!(%local, "web UI / API listening");
    Ok((local, task))
}
