//! Late classifications waiting to be back-filled into stored flows.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use tmon_core::app::AppInfo;
use tmon_core::naming::ConnKey;

/// Most entries kept between drains; more are dropped and counted.
pub const MAX_PENDING: usize = 10_000;

#[derive(Default)]
pub struct PendingApps {
    items: Mutex<Vec<(ConnKey, Arc<AppInfo>)>>,
    dropped: AtomicU64,
}

impl PendingApps {
    pub fn push(&self, key: ConnKey, info: Arc<AppInfo>) {
        let mut v = self.items.lock().unwrap_or_else(|e| e.into_inner());
        if v.len() < MAX_PENDING {
            v.push((key, info));
        } else {
            self.dropped.fetch_add(1, Relaxed);
        }
    }

    pub fn drain(&self) -> Vec<(ConnKey, Arc<AppInfo>)> {
        std::mem::take(&mut *self.items.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Relaxed)
    }
}
