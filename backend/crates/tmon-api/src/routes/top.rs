//! `GET /top/{hosts,remotes,asns,countries,ports}` (FR-015).

use axum::extract::{RawQuery, State};
use tmon_store::duck::query::router::{Dim, Filter};
use tmon_store::duck::query::top::{self, Ctx, TopHost, TopItem, TopRemote};

use crate::envelope::{ApiResult, Meta, ok_meta};
use crate::params::Params;
use crate::routes::traffic::sampled;
use crate::state::AppState;

fn parse(s: &AppState, q: Option<String>) -> Result<(Filter, u64), crate::envelope::ApiError> {
    let p = Params::parse(q.as_deref());
    p.allow(&[], true)?;
    Ok((p.filter(s.now_ms())?, p.limit()?))
}

fn meta(s: &AppState, source: &'static str, limit: u64) -> Meta {
    Meta {
        source: Some(source),
        sampled: Some(sampled(s)),
        limit: Some(limit),
        ..Meta::default()
    }
}

pub async fn hosts(State(s): State<AppState>, RawQuery(q): RawQuery) -> ApiResult<Vec<TopHost>> {
    let (f, limit) = parse(&s, q)?;
    let (w, now) = (s.windows(), s.now_ms());
    let (rows, src) = s
        .blocking(move |st| {
            top::top_hosts(
                &Ctx {
                    conn: &st.conn()?,
                    raw_dir: st.raw_dir(),
                    windows: w,
                    now_ms: now,
                },
                &f,
                limit,
            )
        })
        .await?;
    ok_meta(rows, meta(&s, src, limit))
}

pub async fn remotes(
    State(s): State<AppState>,
    RawQuery(q): RawQuery,
) -> ApiResult<Vec<TopRemote>> {
    let (f, limit) = parse(&s, q)?;
    let (w, now) = (s.windows(), s.now_ms());
    let (rows, src) = s
        .blocking(move |st| {
            top::top_remotes(
                &Ctx {
                    conn: &st.conn()?,
                    raw_dir: st.raw_dir(),
                    windows: w,
                    now_ms: now,
                },
                &f,
                limit,
            )
        })
        .await?;
    ok_meta(rows, meta(&s, src, limit))
}

async fn by(s: AppState, q: Option<String>, dim: Dim) -> ApiResult<Vec<TopItem>> {
    let (f, limit) = parse(&s, q)?;
    let (w, now) = (s.windows(), s.now_ms());
    let (rows, src) = s
        .blocking(move |st| {
            top::top_by(
                &Ctx {
                    conn: &st.conn()?,
                    raw_dir: st.raw_dir(),
                    windows: w,
                    now_ms: now,
                },
                &f,
                dim,
                limit,
            )
        })
        .await?;
    ok_meta(rows, meta(&s, src, limit))
}

pub async fn asns(State(s): State<AppState>, RawQuery(q): RawQuery) -> ApiResult<Vec<TopItem>> {
    by(s, q, Dim::Asn).await
}

pub async fn countries(
    State(s): State<AppState>,
    RawQuery(q): RawQuery,
) -> ApiResult<Vec<TopItem>> {
    by(s, q, Dim::Country).await
}

pub async fn ports(State(s): State<AppState>, RawQuery(q): RawQuery) -> ApiResult<Vec<TopItem>> {
    by(s, q, Dim::Port).await
}
