//! Small sharded LRU used by the app table and the tap's per-key state.

use lru::LruCache;
use std::hash::{BuildHasher, Hash};
use std::num::NonZeroUsize;
use std::sync::{Mutex, MutexGuard};

pub const SHARDS: usize = 16;

pub struct Sharded<K, V> {
    shards: Vec<Mutex<LruCache<K, V>>>,
    hasher: std::collections::hash_map::RandomState,
}

impl<K: Hash + Eq, V> Sharded<K, V> {
    /// `capacity` is the total across shards (at least one per shard).
    pub fn new(capacity: usize) -> Self {
        let per = NonZeroUsize::new(capacity.div_ceil(SHARDS).max(1)).expect("non-zero");
        Self {
            shards: (0..SHARDS)
                .map(|_| Mutex::new(LruCache::new(per)))
                .collect(),
            hasher: Default::default(),
        }
    }

    pub fn shard(&self, k: &K) -> MutexGuard<'_, LruCache<K, V>> {
        let i = (self.hasher.hash_one(k) as usize) % SHARDS;
        self.shards[i].lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).len())
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        for s in &self.shards {
            s.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
    }
}
