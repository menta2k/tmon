//! DNS-learned names waiting to be written to `remote_hostname` (FR-012).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

/// Most names handed over per drain.
pub const MAX_PER_DRAIN: usize = 10_000;

/// Latest name per remote IP; duplicates collapse, the buffer stays bounded.
#[derive(Debug, Default)]
pub struct PendingRemoteNames {
    inner: Mutex<HashMap<IpAddr, Arc<str>>>,
}

impl PendingRemoteNames {
    pub fn push(&self, ip: IpAddr, host: Arc<str>) {
        let mut m = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() < MAX_PER_DRAIN || m.contains_key(&ip) {
            m.insert(ip, host);
        }
    }

    /// Take everything collected so far (at most [`MAX_PER_DRAIN`] entries).
    pub fn drain(&self) -> Vec<(IpAddr, Arc<str>)> {
        let mut m = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *m).into_iter().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
