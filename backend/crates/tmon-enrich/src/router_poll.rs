//! Background pollers for the router integration (RouterOS or OpenWrt);
//! follow live settings, off by default.

use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tmon_store::DuckStore;
use tmon_store::duck::repo::settings::{Router, Settings};
use tmon_store::duck::repo::{hosts, remote_hostnames};

use crate::Pipeline;
use crate::hosts::HostTracker;
use crate::router_source::RouterSource;
use crate::routeros::{DnsEntry, Lease, RouterError, RouterStatus};

const TICK: Duration = Duration::from_secs(1);

pub struct RouterDeps {
    pub store: Arc<DuckStore>,
    pub pipeline: Arc<Pipeline>,
    pub hosts: Arc<HostTracker>,
    pub settings: Arc<ArcSwap<Settings>>,
    pub status: Arc<ArcSwap<RouterStatus>>,
    pub clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

/// Remote IP → hostname map from the store (startup and after each DNS poll).
pub fn load_remote_names(store: &DuckStore) -> tmon_store::StoreResult<HashMap<IpAddr, Arc<str>>> {
    Ok(remote_hostnames::all(&store.conn()?)?
        .into_iter()
        .filter_map(|(ip, h)| Some((ip.parse().ok()?, Arc::from(h))))
        .collect())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> tmon_store::StoreResult<T> + Send + 'static,
) -> Result<T, RouterError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| RouterError::Decode(e.to_string()))?
        .map_err(|e| RouterError::Decode(e.to_string()))
}

async fn apply_leases(d: &RouterDeps, leases: Vec<Lease>) -> Result<(), RouterError> {
    let rows: Vec<_> = leases
        .into_iter()
        .map(|l| (l.ip.to_string(), l.mac, l.hostname))
        .collect();
    let (store, pipeline, tracker, now) = (
        d.store.clone(),
        d.pipeline.clone(),
        d.hosts.clone(),
        (d.clock)(),
    );
    blocking(move || {
        store.with_writer(|c| hosts::apply_leases(c, &rows, now))?;
        tracker.reload_names()?;
        crate::ruleset_handle::reload(&store, &pipeline).map(|_| ())
    })
    .await
}

async fn apply_dns(d: &RouterDeps, entries: Vec<DnsEntry>) -> Result<(), RouterError> {
    let rows: Vec<_> = entries
        .into_iter()
        .map(|e| (e.ip.to_string(), e.name))
        .collect();
    let (store, pipeline, now) = (d.store.clone(), d.pipeline.clone(), (d.clock)());
    blocking(move || {
        store.with_writer(|c| remote_hostnames::upsert(c, &rows, now))?;
        pipeline.set_remote_names(load_remote_names(&store)?);
        // Flows exported before this poll learned their names.
        let rules = pipeline.rules();
        let filled = tmon_store::duck::backfill::backfill_hostnames(&store, now, rules.as_ref())?;
        if filled.named > 0 {
            tracing::debug!(
                named = filled.named,
                recategorized = filled.recategorized,
                "back-filled remote host names"
            );
        }
        Ok(())
    })
    .await
}

fn record(d: &RouterDeps, enabled: bool, result: Result<(), RouterError>) {
    let cur = d.status.load();
    let next = match result {
        Ok(()) => RouterStatus {
            enabled,
            last_ok: Some((d.clock)()),
            error: None,
        },
        Err(e) => {
            if cur.error.as_deref() != Some(&e.to_string()) {
                tracing::warn!(error = %e, "router integration failing");
            }
            RouterStatus {
                enabled,
                last_ok: cur.last_ok,
                error: Some(e.to_string()),
            }
        }
    };
    d.status.store(Arc::new(next));
}

/// One connectivity check used by `POST /settings/router/test`.
pub async fn test_connection(cfg: &Router) -> Result<usize, RouterError> {
    RouterSource::new(cfg)?.leases().await.map(|l| l.len())
}

/// Mark the integration enabled as soon as a (new) config is picked up, so
/// `/status` does not report "disabled" until the first poll finishes.
fn started(d: &RouterDeps, cfg: &Router) {
    let target = crate::routeros::display_target(&cfg.url);
    tracing::info!(kind = ?cfg.kind, %target, "router integration enabled");
    d.status.store(Arc::new(RouterStatus {
        enabled: true,
        ..RouterStatus::default()
    }));
}

/// One lease poll and, where the router has one, one DNS-cache poll when due.
async fn poll_due(d: &RouterDeps, cfg: &Router, client: &RouterSource, due: &mut (i64, i64)) {
    let now = (d.clock)();
    if now >= due.0 {
        due.0 = now + cfg.lease_poll_secs as i64 * 1000;
        let r = match client.leases().await {
            Ok(l) => apply_leases(d, l).await,
            Err(e) => Err(e),
        };
        record(d, true, r);
    }
    if now >= due.1 && cfg.kind.supports_dns_cache() {
        due.1 = now + cfg.dns_poll_secs as i64 * 1000;
        let r = match client.dns_cache().await {
            Ok(Some(e)) => apply_dns(d, e).await,
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        };
        record(d, true, r);
    }
}

/// Poll loop: reacts to settings changes without restart.
pub fn spawn(d: RouterDeps) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut current: Option<(Router, RouterSource)> = None;
        let mut due = (0i64, 0i64);
        loop {
            let cfg = d.settings.load().router.clone();
            if !cfg.enabled {
                if d.status.load().enabled {
                    tracing::info!("router integration disabled");
                    d.status.store(Arc::new(RouterStatus::default()));
                }
                current = None;
                tokio::time::sleep(TICK).await;
                continue;
            }
            if current.as_ref().is_none_or(|(c, _)| *c != cfg) {
                match RouterSource::new(&cfg) {
                    Ok(client) => {
                        started(&d, &cfg);
                        current = Some((cfg.clone(), client));
                    }
                    Err(e) => {
                        record(&d, true, Err(e));
                        tokio::time::sleep(TICK * 5).await;
                        continue;
                    }
                }
                due = (0, 0);
            }
            if let Some((_, client)) = &current {
                poll_due(&d, &cfg, client, &mut due).await;
            }
            tokio::time::sleep(TICK).await;
        }
    })
}
