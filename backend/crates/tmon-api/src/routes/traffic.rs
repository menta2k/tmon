//! `GET /traffic/timeseries`, `GET /traffic/breakdown` (FR-014).

use axum::extract::{RawQuery, State};
use tmon_store::duck::query::router::Dim;
use tmon_store::duck::query::traffic::{self, BreakdownRow, TimeSeries};

use crate::envelope::{ApiError, ApiResult, Meta, ok_meta};
use crate::params::Params;
use crate::state::AppState;

const SERIES_DIMS: [Dim; 5] = [
    Dim::Category,
    Dim::Direction,
    Dim::Proto,
    Dim::Country,
    Dim::Exporter,
];
const BREAKDOWN_DIMS: [Dim; 7] = [
    Dim::Category,
    Dim::Proto,
    Dim::Direction,
    Dim::Country,
    Dim::Asn,
    Dim::Rule,
    Dim::App,
];

/// Parse a dimension parameter restricted to `allowed`.
pub fn dim_param(p: &Params, key: &str, allowed: &[Dim], default: Dim) -> Result<Dim, ApiError> {
    match p.one(key) {
        None => Ok(default),
        Some(v) => allowed
            .iter()
            .copied()
            .find(|d| d.as_str() == v)
            .ok_or_else(|| {
                let names: Vec<&str> = allowed.iter().map(Dim::as_str).collect();
                ApiError::field(key, &format!("must be one of {}", names.join("|")))
            }),
    }
}

/// `meta.sampled`: true when any known exporter reports sampling > 1.
pub fn sampled(s: &AppState) -> bool {
    s.registry.all().iter().any(|e| e.sampling_rate() > 1)
}

pub async fn timeseries(State(s): State<AppState>, RawQuery(q): RawQuery) -> ApiResult<TimeSeries> {
    let p = Params::parse(q.as_deref());
    p.allow(&["group_by"], true)?;
    let group = dim_param(&p, "group_by", &SERIES_DIMS, Dim::Category)?;
    let (now, w) = (s.now_ms(), s.windows());
    let f = p.filter(now)?;
    let step = p.step_ms()?;
    let ts = s
        .blocking(move |st| traffic::timeseries(&st.conn()?, st.raw_dir(), &f, group, step, w, now))
        .await?;
    let meta = Meta {
        source: Some(ts.source),
        sampled: Some(sampled(&s)),
        ..Meta::default()
    };
    ok_meta(ts, meta)
}

pub async fn breakdown(
    State(s): State<AppState>,
    RawQuery(q): RawQuery,
) -> ApiResult<Vec<BreakdownRow>> {
    let p = Params::parse(q.as_deref());
    p.allow(&["by"], true)?;
    let by = dim_param(&p, "by", &BREAKDOWN_DIMS, Dim::Category)?;
    let (now, w) = (s.now_ms(), s.windows());
    let f = p.filter(now)?;
    let limit = p.limit()? as usize;
    let (rows, source) = s
        .blocking(move |st| traffic::breakdown(&st.conn()?, st.raw_dir(), &f, by, w, now))
        .await?;
    let meta = Meta {
        source: Some(source),
        sampled: Some(sampled(&s)),
        ..Meta::default()
    };
    ok_meta(rows.into_iter().take(limit.max(1)).collect(), meta)
}
