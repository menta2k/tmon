//! Single local host drill-down (FR-015, US2).

use duckdb::Connection;
use serde::Serialize;
use std::path::Path;

use super::router::{Dim, Filter, Windows};
use super::top::{Ctx, TopRemote, top_remotes};
use super::traffic::{BreakdownRow, breakdown};
use crate::StoreResult;
use crate::duck::repo::hosts::{self, LocalHost};

pub const HOST_TOP_REMOTES: u64 = 20;
/// Applications listed in a host drill-down (contracts/http-api.md).
pub const HOST_TOP_APPS: usize = 10;

/// One classified application of a host.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopApp {
    pub app: String,
    pub bytes: u64,
    pub flows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Totals {
    pub bytes_down: u64,
    pub bytes_up: u64,
    pub flows: u64,
    pub packets: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostDetail {
    pub host: LocalHost,
    pub totals: Totals,
    pub categories: Vec<BreakdownRow>,
    pub protocols: Vec<BreakdownRow>,
    pub top_remotes: Vec<TopRemote>,
    /// Raw ranges only; empty otherwise (feature 003).
    pub top_apps: Vec<TopApp>,
}

fn totals(dirs: &[BreakdownRow]) -> Totals {
    let get = |k: &str| dirs.iter().find(|r| r.key == k).map_or(0, |r| r.bytes);
    Totals {
        bytes_down: get("down"),
        bytes_up: get("up"),
        flows: dirs.iter().map(|r| r.flows).sum(),
        packets: dirs.iter().map(|r| r.packets).sum(),
    }
}

/// Drill-down for `ip` (must be a known local host or have traffic in range).
pub fn host_detail(cx: &Ctx, ip: &str, f: &Filter) -> StoreResult<(HostDetail, &'static str)> {
    let f = Filter {
        hosts: vec![ip.to_string()],
        ..f.clone()
    };
    let host = hosts::get(cx.conn, ip)?;
    let w: Windows = cx.windows;
    let (dirs, source) = breakdown(cx.conn, cx.raw_dir, &f, Dim::Direction, w, cx.now_ms)?;
    let (categories, _) = breakdown(cx.conn, cx.raw_dir, &f, Dim::Category, w, cx.now_ms)?;
    // Protocol split is not in rollup_host; fall back gracefully when outside raw retention.
    let protocols = match breakdown(cx.conn, cx.raw_dir, &f, Dim::Proto, w, cx.now_ms) {
        Ok((p, _)) => p,
        Err(crate::StoreError::UnsupportedFilterRange { .. }) => Vec::new(),
        Err(e) => return Err(e),
    };
    let top = match top_remotes(cx, &f, HOST_TOP_REMOTES) {
        Ok((t, _)) => t,
        Err(crate::StoreError::UnsupportedFilterRange { .. }) => Vec::new(),
        Err(e) => return Err(e),
    };
    let apps = top_apps(cx.conn, cx.raw_dir, ip, &f, w, cx.now_ms)?;
    Ok((
        HostDetail {
            host,
            totals: totals(&dirs),
            categories,
            protocols,
            top_remotes: top,
            top_apps: apps,
        },
        source,
    ))
}

/// Up to [`HOST_TOP_APPS`] classified applications of `ip` by bytes.
/// Empty when the range is beyond raw retention (apps live in raw flows only).
pub fn top_apps(
    conn: &Connection,
    raw_dir: &Path,
    ip: &str,
    f: &Filter,
    w: Windows,
    now_ms: i64,
) -> StoreResult<Vec<TopApp>> {
    let f = Filter {
        hosts: vec![ip.to_string()],
        ..f.clone()
    };
    let rows = match breakdown(conn, raw_dir, &f, Dim::App, w, now_ms) {
        Ok((rows, _)) => rows,
        Err(crate::StoreError::UnsupportedFilterRange { .. }) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(rows
        .into_iter()
        .filter(|r| !r.key.is_empty())
        .take(HOST_TOP_APPS)
        .map(|r| TopApp {
            app: r.key,
            bytes: r.bytes,
            flows: r.flows,
        })
        .collect())
}
