//! Named connections and DNS device bindings (R8, R12): sharded, bounded,
//! read by the flow pipeline through [`ConnectionNamer`].

use lru::LruCache;
use std::hash::{BuildHasher, Hash};
use std::net::IpAddr;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard};
use tmon_core::naming::{ConnKey, ConnName, ConnectionNamer, NameLookup};

use crate::health::{Health, inc};
use crate::settings::Limits;

pub const SHARDS: usize = 16;

/// Device binding key: (LAN device, remote address).
pub type BindingKey = (IpAddr, IpAddr);

#[derive(Debug, Clone)]
struct ConnEntry {
    name: Arc<ConnName>,
    last_used_ms: i64,
}

#[derive(Debug, Clone)]
struct BindingEntry {
    name: Arc<ConnName>,
    expires_ms: i64,
}

type Shard<K, V> = Mutex<LruCache<K, V>>;

fn shards<K: Hash + Eq, V>(capacity: usize) -> Vec<Shard<K, V>> {
    let per = NonZeroUsize::new(capacity.div_ceil(SHARDS).max(1)).expect("non-zero");
    (0..SHARDS)
        .map(|_| Mutex::new(LruCache::new(per)))
        .collect()
}

fn lock<K: Hash + Eq, V>(s: &Shard<K, V>) -> MutexGuard<'_, LruCache<K, V>> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct ConnTable {
    conns: Vec<Shard<ConnKey, ConnEntry>>,
    bindings: Vec<Shard<BindingKey, BindingEntry>>,
    hasher: std::collections::hash_map::RandomState,
    conn_ttl_ms: i64,
    health: Arc<Health>,
}

impl ConnTable {
    pub fn new(limits: &Limits, health: Arc<Health>) -> Self {
        Self {
            conns: shards(limits.max_connections),
            bindings: shards(limits.max_device_bindings),
            hasher: Default::default(),
            conn_ttl_ms: limits.conn_ttl_ms,
            health,
        }
    }

    fn shard_of<K: Hash>(&self, k: &K) -> usize {
        (self.hasher.hash_one(k) as usize) % SHARDS
    }

    /// Publish (or replace) the name of a connection.
    pub fn insert(&self, key: ConnKey, name: Arc<ConnName>, now_ms: i64) {
        let mut s = lock(&self.conns[self.shard_of(&key)]);
        let entry = ConnEntry {
            name,
            last_used_ms: now_ms,
        };
        if let Some((old, _)) = s.push(key, entry)
            && old != key
        {
            inc(&self.health.counters.conn_evicted_lru);
        }
    }

    /// Bind `lan`'s view of `remote` to a DNS-learned name (newer replaces older).
    pub fn bind(&self, lan: IpAddr, remote: IpAddr, name: Arc<ConnName>, expires_ms: i64) {
        let key = (lan, remote);
        let mut s = lock(&self.bindings[self.shard_of(&key)]);
        if let Some((old, _)) = s.push(key, BindingEntry { name, expires_ms })
            && old != key
        {
            inc(&self.health.counters.bindings_evicted);
        }
    }

    fn conn(&self, key: &ConnKey, now_ms: i64) -> Option<Arc<ConnName>> {
        let mut s = lock(&self.conns[self.shard_of(key)]);
        let e = s.get_mut(key)?;
        if e.last_used_ms + self.conn_ttl_ms < now_ms {
            s.pop(key);
            inc(&self.health.counters.conn_evicted_ttl);
            return None;
        }
        e.last_used_ms = e.last_used_ms.max(now_ms);
        Some(Arc::clone(&e.name))
    }

    fn binding(&self, key: &BindingKey, now_ms: i64) -> Option<Arc<ConnName>> {
        let mut s = lock(&self.bindings[self.shard_of(key)]);
        let e = s.get(key)?;
        if e.expires_ms < now_ms {
            s.pop(key);
            inc(&self.health.counters.bindings_evicted);
            return None;
        }
        Some(Arc::clone(&e.name))
    }

    /// Drop expired entries; returns how many were removed.
    pub fn sweep(&self, now_ms: i64) -> usize {
        let mut removed = 0;
        for shard in &self.conns {
            let mut s = lock(shard);
            while s
                .peek_lru()
                .is_some_and(|(_, e)| e.last_used_ms + self.conn_ttl_ms < now_ms)
            {
                s.pop_lru();
                inc(&self.health.counters.conn_evicted_ttl);
                removed += 1;
            }
        }
        for shard in &self.bindings {
            let mut s = lock(shard);
            let expired: Vec<BindingKey> = s
                .iter()
                .filter(|(_, e)| e.expires_ms < now_ms)
                .map(|(k, _)| *k)
                .collect();
            for k in expired {
                s.pop(&k);
                inc(&self.health.counters.bindings_evicted);
                removed += 1;
            }
        }
        removed
    }

    pub fn conns_len(&self) -> usize {
        self.conns.iter().map(|s| lock(s).len()).sum()
    }

    pub fn bindings_len(&self) -> usize {
        self.bindings.iter().map(|s| lock(s).len()).sum()
    }
}

impl ConnectionNamer for ConnTable {
    fn lookup(&self, key: &ConnKey, now_ms: i64) -> NameLookup {
        let connection = self.conn(key, now_ms);
        let device = self.binding(&(key.lan_ip, key.remote_ip), now_ms);
        let c = &self.health.counters;
        inc(&c.lookups);
        if connection.is_some() {
            inc(&c.lookup_hits_conn);
        } else if device.is_some() {
            inc(&c.lookup_hits_dns);
        }
        let hit = connection.is_some() || device.is_some();
        self.health.record(now_ms, |b| {
            b.lookups += 1;
            b.lookup_hits += u64::from(hit);
        });
        NameLookup { connection, device }
    }
}
