//! The router API selected by `router.kind`: RouterOS REST or OpenWrt ubus.

use tmon_store::duck::repo::settings::{Router, RouterKind};

use crate::openwrt::OpenWrtClient;
use crate::routeros::{DnsEntry, Lease, RouterClient, RouterError};

pub enum RouterSource {
    RouterOs(RouterClient),
    OpenWrt(OpenWrtClient),
}

impl RouterSource {
    pub fn new(cfg: &Router) -> Result<Self, RouterError> {
        Ok(match cfg.kind {
            RouterKind::RouterOs => Self::RouterOs(RouterClient::new(cfg)?),
            RouterKind::OpenWrt => Self::OpenWrt(OpenWrtClient::new(cfg)?),
        })
    }

    pub async fn leases(&self) -> Result<Vec<Lease>, RouterError> {
        match self {
            Self::RouterOs(c) => c.leases().await,
            Self::OpenWrt(c) => c.leases().await,
        }
    }

    /// `None` when the router has no DNS-cache API.
    pub async fn dns_cache(&self) -> Result<Option<Vec<DnsEntry>>, RouterError> {
        match self {
            Self::RouterOs(c) => c.dns_cache().await.map(Some),
            Self::OpenWrt(_) => Ok(None),
        }
    }
}
