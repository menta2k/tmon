//! Classifications by connection (data-model.md §5): a bounded, sharded
//! LRU with an idle TTL; the pipeline reads it as a [`ConnectionClassifier`].

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use tmon_core::app::{AppInfo, ConnectionClassifier};
use tmon_core::naming::ConnKey;

use crate::lru_shards::Sharded;

#[derive(Clone)]
struct Entry {
    info: Arc<AppInfo>,
    last_used_ms: i64,
}

pub struct AppTable {
    entries: Sharded<ConnKey, Entry>,
    ttl_ms: i64,
    evicted: AtomicU64,
}

/// Outcome of [`AppTable::insert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert {
    /// New or better answer stored.
    Stored,
    /// Not better than the stored answer (FR-006); ignored.
    Kept,
}

impl AppTable {
    pub fn new(capacity: usize, ttl_ms: i64) -> Self {
        Self {
            entries: Sharded::new(capacity),
            ttl_ms,
            evicted: AtomicU64::new(0),
        }
    }

    /// Store `info` unless the current, unexpired answer is at least as good.
    pub fn insert(&self, key: ConnKey, info: Arc<AppInfo>, now_ms: i64) -> Insert {
        let mut s = self.entries.shard(&key);
        let live = s
            .peek(&key)
            .filter(|e| e.last_used_ms + self.ttl_ms >= now_ms)
            .map(|e| Arc::clone(&e.info));
        if live.is_some_and(|old| !info.better_than(&old)) {
            return Insert::Kept;
        }
        let entry = Entry {
            info,
            last_used_ms: now_ms,
        };
        if let Some((old, _)) = s.push(key, entry)
            && old != key
        {
            self.evicted.fetch_add(1, Relaxed);
        }
        Insert::Stored
    }

    pub fn get(&self, key: &ConnKey, now_ms: i64) -> Option<Arc<AppInfo>> {
        let mut s = self.entries.shard(key);
        let e = s.get_mut(key)?;
        if e.last_used_ms + self.ttl_ms < now_ms {
            s.pop(key);
            return None;
        }
        e.last_used_ms = now_ms;
        Some(Arc::clone(&e.info))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn evicted(&self) -> u64 {
        self.evicted.load(Relaxed)
    }
}

impl ConnectionClassifier for AppTable {
    fn classify(&self, key: &ConnKey, now_ms: i64) -> Option<Arc<AppInfo>> {
        self.get(key, now_ms)
    }
}
