//! MaxMind-format (DB-IP Lite / GeoLite2) country + ASN lookup with hot reload and an LRU cache.

use arc_swap::ArcSwapOption;
use lru::LruCache;
use maxminddb::{Reader, geoip2};
use std::net::IpAddr;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::geo::{GeoInfo, GeoLookup, GeoStatus, is_private};

pub const CACHE_ENTRIES: usize = 65_536;
pub const RELOAD_CHECK: Duration = Duration::from_secs(60);

struct Db {
    reader: Reader<Vec<u8>>,
    mtime: Option<SystemTime>,
}

impl Db {
    fn open(path: &Path) -> Option<Db> {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        match Reader::open_readfile(path) {
            Ok(reader) => Some(Db { reader, mtime }),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "GeoIP database not loaded");
                None
            }
        }
    }

    fn date(&self) -> Option<String> {
        let t =
            time::OffsetDateTime::from_unix_timestamp(self.reader.metadata().build_epoch as i64)
                .ok()?;
        Some(format!(
            "{:04}-{:02}-{:02}",
            t.year(),
            t.month() as u8,
            t.day()
        ))
    }
}

/// Hot-reloadable GeoIP lookup.
pub struct GeoIp {
    country_path: PathBuf,
    asn_path: PathBuf,
    country: ArcSwapOption<Db>,
    asn: ArcSwapOption<Db>,
    cache: Mutex<LruCache<IpAddr, GeoInfo>>,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl GeoIp {
    /// Open both databases; missing files are tolerated (lookups return empty info).
    pub fn open(country_path: &Path, asn_path: &Path) -> Arc<Self> {
        let cap = NonZeroUsize::new(CACHE_ENTRIES).unwrap_or(NonZeroUsize::MIN);
        Arc::new(Self {
            country: ArcSwapOption::from(Db::open(country_path).map(Arc::new)),
            asn: ArcSwapOption::from(Db::open(asn_path).map(Arc::new)),
            country_path: country_path.to_path_buf(),
            asn_path: asn_path.to_path_buf(),
            cache: Mutex::new(LruCache::new(cap)),
        })
    }

    /// Reload any database whose file changed; returns true when something was reloaded.
    pub fn reload_if_changed(&self) -> bool {
        let mut changed = false;
        for (slot, path) in [
            (&self.country, &self.country_path),
            (&self.asn, &self.asn_path),
        ] {
            let current = slot.load().as_ref().and_then(|d| d.mtime);
            let on_disk = mtime(path);
            if on_disk.is_some() && on_disk != current {
                if let Some(db) = Db::open(path) {
                    tracing::info!(path = %path.display(), "GeoIP database reloaded");
                    slot.store(Some(Arc::new(db)));
                    changed = true;
                }
            }
        }
        if changed {
            if let Ok(mut c) = self.cache.lock() {
                c.clear();
            }
        }
        changed
    }

    /// Periodically check the files for replacement.
    pub fn spawn_watcher(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let mut t = tokio::time::interval(RELOAD_CHECK);
            loop {
                t.tick().await;
                let m = Arc::clone(&me);
                if let Err(e) = tokio::task::spawn_blocking(move || m.reload_if_changed()).await {
                    tracing::error!(error = %e, "GeoIP reload task failed");
                }
            }
        })
    }

    fn resolve(&self, ip: IpAddr) -> GeoInfo {
        let country = self.country.load().as_ref().and_then(|db| {
            let r = db.reader.lookup(ip).ok()?;
            let c: geoip2::Country = r.decode().ok()??;
            c.country
                .iso_code
                .map(|s| Arc::from(s.to_ascii_uppercase()))
        });
        let (asn, as_name) = self
            .asn
            .load()
            .as_ref()
            .and_then(|db| {
                let r = db.reader.lookup(ip).ok()?;
                let a: geoip2::Asn = r.decode().ok()??;
                Some((
                    a.autonomous_system_number,
                    a.autonomous_system_organization.map(Arc::from),
                ))
            })
            .unwrap_or((None, None));
        GeoInfo {
            country,
            asn,
            as_name,
        }
    }

    pub fn cached(&self) -> usize {
        self.cache.lock().map(|c| c.len()).unwrap_or(0)
    }
}

impl GeoLookup for GeoIp {
    fn lookup(&self, ip: IpAddr) -> GeoInfo {
        if is_private(&ip) {
            return GeoInfo::default();
        }
        if let Some(hit) = self.cache.lock().ok().and_then(|mut c| c.get(&ip).cloned()) {
            return hit;
        }
        let info = self.resolve(ip);
        if let Ok(mut c) = self.cache.lock() {
            c.put(ip, info.clone());
        }
        info
    }

    fn status(&self) -> GeoStatus {
        let (c, a) = (self.country.load(), self.asn.load());
        GeoStatus {
            loaded: c.is_some() && a.is_some(),
            db_date: c.as_ref().or(a.as_ref()).and_then(|d| d.date()),
        }
    }
}
