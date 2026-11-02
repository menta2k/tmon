//! `GET /flows` and `GET /flows/export.csv` (FR-018).

use axum::body::Body;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use tmon_store::duck::query::flows::{
    self, CSV_HEADER, EXPORT_CAP, FlowRow, MAX_PAGE, PageReq, Sort, SortKey,
};
use tmon_store::duck::query::router::Filter;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::envelope::{ApiError, ApiResult, Meta, ok_meta};
use crate::params::Params;
use crate::state::AppState;

pub const DEFAULT_LIMIT: u64 = 100;
/// Rows per streamed CSV chunk.
const CHUNK_ROWS: usize = 1000;

/// Common filter plus `remote_ip` / `port` (remote service port) / `app`.
fn filter(s: &AppState, p: &Params) -> Result<Filter, ApiError> {
    let f = p.filter(s.now_ms())?;
    let ports = p
        .all("port")
        .iter()
        .map(|v| {
            v.trim()
                .parse::<u16>()
                .map_err(|_| ApiError::field("port", "must be 0–65535"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Filter {
        remote_ips: p.ips("remote_ip")?,
        ports,
        apps: p.apps()?,
        ..f
    })
}

/// `sort=time|bytes|packets` and `order=desc|asc` (default newest first).
fn sort(p: &Params) -> Result<Sort, ApiError> {
    let key = match p.one("sort") {
        None => SortKey::default(),
        Some(v) => SortKey::parse(v.trim())
            .ok_or_else(|| ApiError::field("sort", "must be time|bytes|packets"))?,
    };
    let desc = match p.one("order").map(str::trim) {
        None | Some("desc") => true,
        Some("asc") => false,
        Some(_) => return Err(ApiError::field("order", "must be asc|desc")),
    };
    Ok(Sort { key, desc })
}

pub async fn search(State(s): State<AppState>, RawQuery(q): RawQuery) -> ApiResult<Vec<FlowRow>> {
    let p = Params::parse(q.as_deref());
    p.allow(&["remote_ip", "port", "page", "app", "sort", "order"], true)?;
    let f = filter(&s, &p)?;
    let page = p.u64_or("page", 1, 1, u64::from(u32::MAX))?;
    let limit = p.u64_or("limit", DEFAULT_LIMIT, 1, MAX_PAGE)?;
    let req = PageReq {
        page,
        limit,
        sort: sort(&p)?,
    };
    let (w, now) = (s.windows(), s.now_ms());
    let (rows, total) = s
        .blocking(move |st| flows::search(&st.conn()?, st.raw_dir(), &f, req, w, now))
        .await?;
    ok_meta(
        rows,
        Meta {
            source: Some("raw"),
            page: Some(page),
            limit: Some(limit),
            total: Some(total),
            ..Meta::default()
        },
    )
}

fn csv_line(fields: &[String]) -> Result<Vec<u8>, String> {
    let mut w = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    w.write_record(fields).map_err(|e| e.to_string())?;
    w.into_inner().map_err(|e| e.to_string())
}

fn stamp(ms: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000))
        .map(|t| {
            format!(
                "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
                t.year(),
                u8::from(t.month()),
                t.day(),
                t.hour(),
                t.minute(),
                t.second()
            )
        })
        .unwrap_or_else(|_| ms.to_string())
}

pub async fn export(
    State(s): State<AppState>,
    RawQuery(q): RawQuery,
) -> Result<Response, ApiError> {
    let p = Params::parse(q.as_deref());
    p.allow(&["remote_ip", "port", "app", "sort", "order"], true)?;
    let f = filter(&s, &p)?;
    let order = sort(&p)?;
    let (w, now) = (s.windows(), s.now_ms());
    if f.from_ms < now - w.raw_ms {
        return Err(ApiError::UnsupportedFilterRange(Vec::new()));
    }
    let name = format!("tmon-flows-{}-{}.csv", stamp(f.from_ms), stamp(f.to_ms));
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(8);
    let store = s.store.clone();
    tokio::task::spawn_blocking(move || {
        let header: Vec<String> = CSV_HEADER.iter().map(|h| h.to_string()).collect();
        let mut buf = csv_line(&header).unwrap_or_default();
        let mut pending = 0;
        let send = |b: Vec<u8>| {
            tx.blocking_send(Ok(Bytes::from(b)))
                .map_err(|_| tmon_store::StoreError::Task("client went away".into()))
        };
        let result = store.conn().and_then(|c| {
            flows::export(
                &c,
                store.raw_dir(),
                &f,
                (EXPORT_CAP, order),
                w,
                now,
                |row| {
                    buf.extend(csv_line(&row.csv_fields()).map_err(tmon_store::StoreError::Task)?);
                    pending += 1;
                    if pending >= CHUNK_ROWS {
                        pending = 0;
                        send(std::mem::take(&mut buf))?;
                    }
                    Ok(())
                },
            )
        });
        match result {
            Ok((_, truncated)) => {
                if truncated {
                    buf.extend(format!("# truncated at {EXPORT_CAP} rows\n").into_bytes());
                }
                let _ = send(buf);
            }
            Err(e) => {
                tracing::error!(error = %e, "CSV export failed");
                let _ = tx.blocking_send(Err(std::io::Error::other("export failed")));
            }
        }
    });
    let mut res = Body::from_stream(ReceiverStream::new(rx)).into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    let disp = HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    h.insert(header::CONTENT_DISPOSITION, disp);
    Ok(res)
}
