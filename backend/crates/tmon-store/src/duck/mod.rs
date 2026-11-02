//! DuckDB + Parquet implementation of the storage tier.

pub mod backfill;
pub mod backfill_apps;
pub mod files;
pub mod ingest;
pub mod query;
pub mod recategorize;
pub mod repo;
pub mod retention;
pub mod rotate;
pub mod rows;
pub mod schema;
pub mod writer;

pub use duckdb::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tmon_core::FlowRecord;

use crate::{FlowStore, StoreResult};

/// Tunables from `[storage]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreOptions {
    pub memory_limit: String,
    pub threads: u32,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            memory_limit: "512MB".into(),
            threads: 2,
        }
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Handle to the data directory. The ingest connection is serialized by a
/// mutex (single writer); other work uses cloned connections.
pub struct DuckStore {
    writer: Mutex<Connection>,
    dir: PathBuf,
    raw_dir: PathBuf,
}

impl DuckStore {
    pub fn open(dir: &Path, opts: &StoreOptions) -> StoreResult<Self> {
        let raw_dir = dir.join("raw");
        std::fs::create_dir_all(&raw_dir)?;
        let conn = Connection::open(dir.join("tmon.duckdb"))?;
        let limit = opts.memory_limit.replace('\'', "");
        conn.execute_batch(&format!(
            "SET memory_limit = '{limit}'; SET threads = {};",
            opts.threads.max(1)
        ))?;
        schema::migrate(&conn, now_ms())?;
        schema::create_stage(&conn)?;
        let store = Self {
            writer: Mutex::new(conn),
            dir: dir.to_path_buf(),
            raw_dir,
        };
        store.refresh_view()?;
        Ok(store)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn raw_dir(&self) -> &Path {
        &self.raw_dir
    }

    /// A new connection to the same database (for reads / config writes).
    pub fn conn(&self) -> StoreResult<Connection> {
        Ok(self.lock()?.try_clone()?)
    }

    fn lock(&self) -> StoreResult<std::sync::MutexGuard<'_, Connection>> {
        self.writer
            .lock()
            .map_err(|_| crate::StoreError::Task("writer mutex poisoned".into()))
    }

    /// Run `f` with the single writer connection.
    pub fn with_writer<T>(&self, f: impl FnOnce(&Connection) -> StoreResult<T>) -> StoreResult<T> {
        let guard = self.lock()?;
        f(&guard)
    }

    /// (Re)create the `flows` view over the hot table and all Parquet files.
    pub fn refresh_view(&self) -> StoreResult<()> {
        self.refresh_view_excluding(&[])
    }

    /// Recreate the `flows` view without `skip` (before deleting those files).
    pub fn refresh_view_excluding(&self, skip: &[files::RawFile]) -> StoreResult<()> {
        let files: Vec<files::RawFile> =
            files::raw_files(&self.raw_dir, i64::MIN / 2, i64::MAX / 2)?
                .into_iter()
                .filter(|f| !skip.contains(f))
                .collect();
        let sql = format!(
            "CREATE OR REPLACE VIEW flows AS {}",
            files::union_sql(&files)
        );
        self.with_writer(|c| Ok(c.execute_batch(&sql)?))
    }
}

impl FlowStore for DuckStore {
    fn append_batch(&self, flows: &[FlowRecord]) -> StoreResult<()> {
        self.with_writer(|c| ingest::ingest(c, flows))
    }

    fn rotate(&self, now_ms: i64) -> StoreResult<usize> {
        let n = self.with_writer(|c| rotate::rotate(c, &self.raw_dir, now_ms))?;
        if n > 0 {
            self.refresh_view()?;
        }
        Ok(n)
    }
}
