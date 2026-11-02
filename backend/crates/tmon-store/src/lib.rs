//! tmon-store: embedded DuckDB (rollups + config) and hourly Parquet files (raw flows).

pub mod duck;

use tmon_core::{FlowRecord, ValidationErrors};

pub use duck::{DuckStore, StoreOptions};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] duckdb::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation failed: {0}")]
    Validation(#[from] ValidationErrors),
    #[error("filter combination is only available within raw retention")]
    UnsupportedFilterRange { supported_dims: Vec<String> },
    #[error("storage task failed: {0}")]
    Task(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// Write side of flow storage (the query side lives on `DuckStore`).
pub trait FlowStore: Send + Sync {
    /// Append enriched flows (raw tier) and update rollups atomically.
    fn append_batch(&self, flows: &[FlowRecord]) -> StoreResult<()>;
    /// Move completed hours (`< now`'s hour) from the hot table into Parquet.
    fn rotate(&self, now_ms: i64) -> StoreResult<usize>;
}
