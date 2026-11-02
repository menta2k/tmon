//! `GET /status` (FR-019, US5).

use axum::extract::State;
use serde::Serialize;
use std::collections::BTreeMap;
use tmon_collector::ExporterInfo;
use tmon_enrich::routeros::RouterStatus;
use tmon_store::duck::retention;

use crate::envelope::{ApiResult, ok};
use crate::state::AppState;
use crate::warnings::{Warning, current};

#[derive(Serialize)]
pub struct ExporterStatus {
    #[serde(flatten)]
    pub info: ExporterInfo,
    pub flows_per_sec: f64,
    pub received: u64,
    pub decoded: u64,
    pub template_missing: u64,
    pub parse_errors: u64,
    pub dropped: BTreeMap<String, u64>,
}

#[derive(Serialize)]
pub struct Collector {
    pub proto: String,
    pub port: u16,
    pub address: String,
    pub listening: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct Ingest {
    pub flows_per_sec: f64,
    pub dropped_total: BTreeMap<String, u64>,
}

#[derive(Serialize)]
pub struct Storage {
    pub disk_bytes: u64,
    pub cap_bytes: u64,
    pub oldest_raw: Option<i64>,
}

#[derive(Serialize)]
pub struct Enrichment {
    pub geoip_db_date: Option<String>,
    pub router_integration: RouterStatus,
}

#[derive(Serialize)]
pub struct Status {
    pub exporters: Vec<ExporterStatus>,
    pub collectors: Vec<Collector>,
    pub ingest: Ingest,
    pub storage: Storage,
    pub enrichment: Enrichment,
    pub capture: super::capture::CaptureStatus,
    pub dpi: super::dpi::DpiView,
    pub warnings: Vec<Warning>,
}

fn exporters(s: &AppState) -> Vec<ExporterStatus> {
    let rates = s.health.rates();
    s.registry
        .snapshot(s.now_ms(), s.stall_after_ms)
        .into_iter()
        .map(|info| ExporterStatus {
            flows_per_sec: rates.exporter(info.id),
            received: info.counters.received,
            decoded: info.counters.decoded,
            template_missing: info.counters.template_missing,
            parse_errors: info.counters.parse_errors,
            dropped: BTreeMap::new(),
            info,
        })
        .collect()
}

pub async fn status(State(s): State<AppState>) -> ApiResult<Status> {
    let cap = s.policy().cap_bytes;
    let (disk_bytes, oldest_raw) = s
        .blocking(|st| Ok((retention::disk_bytes(st)?, retention::oldest_raw(st)?)))
        .await?;
    let collectors = s
        .listeners
        .load()
        .iter()
        .map(|l| Collector {
            proto: format!("{:?}", l.proto).to_lowercase(),
            port: l.port,
            address: l.address.clone(),
            listening: l.listening,
            error: l.error.clone(),
        })
        .collect();
    let dropped_total = s
        .globals
        .snapshot()
        .into_iter()
        .map(|(r, n)| (r.as_str().to_string(), n))
        .collect();
    ok(Status {
        exporters: exporters(&s),
        collectors,
        ingest: Ingest {
            flows_per_sec: s.health.rates().flows_per_sec(),
            dropped_total,
        },
        storage: Storage {
            disk_bytes,
            cap_bytes: cap,
            oldest_raw,
        },
        enrichment: Enrichment {
            geoip_db_date: s.pipeline.deps().geo.status().db_date,
            router_integration: (**s.router.load()).clone(),
        },
        capture: super::capture::status(s.capture.as_deref(), s.now_ms()),
        dpi: super::dpi::status(s.dpi.as_deref(), s.now_ms()),
        warnings: current(&s),
    })
}
