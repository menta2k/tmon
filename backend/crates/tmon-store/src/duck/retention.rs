//! Retention janitor (FR-020/FR-022): expire raw hours and rollup rows, enforce the disk cap.

use serde::Serialize;
use std::path::Path;

use super::DuckStore;
use super::files::{HOUR_MS, RawFile, raw_files};
use super::schema::{GRAINS, ROLLUPS};
use crate::StoreResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub raw_ms: i64,
    pub rollup_1m_ms: i64,
    pub rollup_1h_ms: i64,
    pub cap_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub expired_files: usize,
    /// Raw hours removed only to satisfy the disk cap.
    pub capped_files: usize,
    pub disk_bytes: u64,
    pub cap_bytes: u64,
    pub oldest_raw: Option<i64>,
}

impl Report {
    pub fn cap_exceeded(&self) -> bool {
        self.capped_files > 0 || self.disk_bytes > self.cap_bytes
    }
}

fn remove(f: &RawFile) -> StoreResult<()> {
    std::fs::remove_file(&f.path)?;
    if let Some(dir) = f.path.parent() {
        if std::fs::read_dir(dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
            std::fs::remove_dir(dir)?;
        }
    }
    Ok(())
}

/// Bytes used by the database files plus all raw Parquet files.
pub fn disk_bytes(store: &DuckStore) -> StoreResult<u64> {
    let db: u64 = std::fs::read_dir(store.dir())?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("tmon.duckdb"))
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    let raw: u64 = all_raw(store.raw_dir())?.iter().map(|f| f.bytes).sum();
    Ok(db + raw)
}

fn all_raw(raw_dir: &Path) -> StoreResult<Vec<RawFile>> {
    raw_files(raw_dir, i64::MIN / 2, i64::MAX / 2)
}

/// Oldest raw data timestamp (start of the oldest Parquet hour, or the hot table minimum).
pub fn oldest_raw(store: &DuckStore) -> StoreResult<Option<i64>> {
    if let Some(f) = all_raw(store.raw_dir())?.first() {
        return Ok(Some(f.hour_ms));
    }
    Ok(store
        .conn()?
        .query_row("SELECT min(ts) FROM flows_hot", [], |r| r.get(0))?)
}

fn expire_rollups(store: &DuckStore, p: &Policy, now_ms: i64) -> StoreResult<()> {
    store.with_writer(|c| {
        for (name, _, _) in ROLLUPS {
            for (grain, _) in GRAINS {
                let keep = if grain == "1m" {
                    p.rollup_1m_ms
                } else {
                    p.rollup_1h_ms
                };
                c.execute(
                    &format!("DELETE FROM {name}_{grain} WHERE bucket < ?"),
                    [now_ms - keep],
                )?;
            }
        }
        c.execute(
            "DELETE FROM rollup_remote_1h WHERE bucket < ?",
            [now_ms - p.rollup_1h_ms],
        )?;
        c.execute("DELETE FROM flows_hot WHERE ts < ?", [now_ms - p.raw_ms])?;
        Ok(())
    })
}

/// One janitor pass.
pub fn run(store: &DuckStore, p: &Policy, now_ms: i64) -> StoreResult<Report> {
    let files = all_raw(store.raw_dir())?;
    let (expired, mut kept): (Vec<RawFile>, Vec<RawFile>) = files
        .into_iter()
        .partition(|f| f.hour_ms + HOUR_MS <= now_ms - p.raw_ms);
    let mut report = Report {
        expired_files: expired.len(),
        cap_bytes: p.cap_bytes,
        ..Report::default()
    };
    if !expired.is_empty() {
        store.refresh_view_excluding(&expired)?;
        expired.iter().try_for_each(remove)?;
    }
    expire_rollups(store, p, now_ms)?;
    let mut used = disk_bytes(store)?;
    let mut capped = Vec::new();
    while used > p.cap_bytes && !kept.is_empty() {
        let f = kept.remove(0);
        used = used.saturating_sub(f.bytes);
        capped.push(f);
    }
    if !capped.is_empty() {
        store.refresh_view_excluding(&capped)?;
        capped.iter().try_for_each(remove)?;
        tracing::warn!(
            files = capped.len(),
            cap = p.cap_bytes,
            "disk cap exceeded: removed oldest raw hours"
        );
    }
    store.refresh_view()?;
    report.capped_files = capped.len();
    report.disk_bytes = disk_bytes(store)?;
    report.oldest_raw = oldest_raw(store)?;
    Ok(report)
}
