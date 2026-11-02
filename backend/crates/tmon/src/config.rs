//! Configuration (contracts/config.md): `tmon.toml` + `TMON_<SECTION>__<KEY>` env.

use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

pub use crate::config_capture::CaptureConfig;
pub use crate::config_dpi::DpiConfig;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub data_dir: PathBuf,
    pub http: HttpConfig,
    pub collector: CollectorConfig,
    pub storage: StorageConfig,
    pub retention: RetentionConfig,
    pub geoip: GeoipConfig,
    pub router: RouterConfig,
    pub capture: CaptureConfig,
    pub dpi: DpiConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub listen: String,
    pub password_hash: String,
    pub api_token: String,
    pub session_ttl_hours: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CollectorConfig {
    pub netflow_listen: Vec<String>,
    pub sflow_listen: Vec<String>,
    pub allowed_exporters: Vec<String>,
    pub socket_recv_buffer_bytes: usize,
    pub queue_capacity: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    pub flush_interval_secs: u64,
    pub memory_limit: String,
    pub threads: u32,
    pub disk_cap_gb: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetentionConfig {
    pub raw_days: u32,
    pub rollup_1m_days: u32,
    pub rollup_1h_days: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeoipConfig {
    pub country_db: PathBuf,
    pub asn_db: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RouterConfig {
    /// `routeros` (default) or `openwrt`.
    pub kind: tmon_store::duck::repo::settings::RouterKind,
    pub enabled: bool,
    pub url: String,
    pub username: String,
    pub password: String,
    pub verify_tls: bool,
    pub lease_poll_secs: u64,
    pub dns_poll_secs: u64,
}

const DEFAULT_DATA_DIR: &str = "/var/lib/tmon";

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(DEFAULT_DATA_DIR),
            http: HttpConfig::default(),
            collector: CollectorConfig::default(),
            storage: StorageConfig::default(),
            retention: RetentionConfig::default(),
            geoip: GeoipConfig::default(),
            router: RouterConfig::default(),
            capture: CaptureConfig::default(),
            dpi: DpiConfig::default(),
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:8080".into(),
            password_hash: String::new(),
            api_token: String::new(),
            session_ttl_hours: 168,
        }
    }
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            netflow_listen: vec!["0.0.0.0:2055".into(), "0.0.0.0:4739".into()],
            sflow_listen: vec!["0.0.0.0:6343".into()],
            allowed_exporters: Vec::new(),
            socket_recv_buffer_bytes: 8 * 1024 * 1024,
            queue_capacity: 65_536,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            flush_interval_secs: 5,
            memory_limit: "512MB".into(),
            threads: 2,
            disk_cap_gb: 20.0,
        }
    }
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            raw_days: 7,
            rollup_1m_days: 30,
            rollup_1h_days: 365,
        }
    }
}

impl Default for GeoipConfig {
    fn default() -> Self {
        let geo = Path::new(DEFAULT_DATA_DIR).join("geo");
        Self {
            country_db: geo.join("dbip-country-lite.mmdb"),
            asn_db: geo.join("dbip-asn-lite.mmdb"),
        }
    }
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            kind: Default::default(),
            enabled: false,
            url: "https://192.168.88.1".into(),
            username: "tmon-ro".into(),
            password: String::new(),
            verify_tls: true,
            lease_poll_secs: 60,
            dns_poll_secs: 30,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("configuration could not be loaded: {0}")]
    Load(String),
    #[error("{0}")]
    Invalid(String),
}

/// Validated, typed configuration plus non-fatal warnings.
#[derive(Debug, Clone)]
pub struct Validated {
    pub config: Config,
    pub http_listen: SocketAddr,
    pub netflow_listen: Vec<SocketAddr>,
    pub sflow_listen: Vec<SocketAddr>,
    pub allowed_exporters: Vec<IpAddr>,
    /// `None` when capture is disabled.
    pub capture: Option<tmon_capture::CaptureSettings>,
    /// `None` when application classification is disabled.
    pub dpi: Option<tmon_dpi_client::DpiSettings>,
    pub warnings: Vec<String>,
}

impl Config {
    /// Load defaults ← optional TOML file ← `TMON_*` environment.
    pub fn load(path: Option<&Path>) -> Result<Config, ConfigError> {
        let mut fig = Figment::from(Serialized::defaults(Config::default()));
        if let Some(p) = path {
            if !p.exists() {
                return Err(ConfigError::Load(format!(
                    "config file {} not found",
                    p.display()
                )));
            }
            fig = fig.merge(Toml::file(p));
        }
        let env = Env::prefixed("TMON_")
            .filter(|k| is_config_key(k.as_str()))
            .split("__");
        fig.merge(env)
            .extract()
            .map_err(|e| ConfigError::Load(e.to_string()))
    }

    /// Startup validation (contracts/config.md "Startup validation").
    pub fn validate(self) -> Result<Validated, ConfigError> {
        if self.http.password_hash.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "set http.password_hash (generate one with `tmon hash-password`)".into(),
            ));
        }
        let http_listen = parse_addr("http.listen", &self.http.listen)?;
        let netflow_listen = parse_all("collector.netflow_listen", &self.collector.netflow_listen)?;
        let sflow_listen = parse_all("collector.sflow_listen", &self.collector.sflow_listen)?;
        let allowed_exporters = self
            .collector
            .allowed_exporters
            .iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| invalid("collector.allowed_exporters", s))
            })
            .collect::<Result<Vec<IpAddr>, _>>()?;
        self.check_ranges()?;
        let capture = self.capture.validate()?;
        let dpi = self.dpi.validate(&self.capture)?;
        ensure_writable(&self.data_dir)?;
        let warnings = self.geoip_warnings();
        Ok(Validated {
            config: self,
            http_listen,
            netflow_listen,
            sflow_listen,
            allowed_exporters,
            capture,
            dpi,
            warnings,
        })
    }

    fn check_ranges(&self) -> Result<(), ConfigError> {
        let r = &self.retention;
        if r.raw_days == 0 || r.rollup_1m_days == 0 || r.rollup_1h_days == 0 {
            return Err(ConfigError::Invalid(
                "retention periods must be at least 1 day".into(),
            ));
        }
        if self.storage.flush_interval_secs == 0 || self.collector.queue_capacity == 0 {
            return Err(ConfigError::Invalid(
                "flush interval and queue capacity must be > 0".into(),
            ));
        }
        if self.storage.disk_cap_gb <= 0.0 {
            return Err(ConfigError::Invalid(
                "storage.disk_cap_gb must be > 0".into(),
            ));
        }
        Ok(())
    }

    fn geoip_warnings(&self) -> Vec<String> {
        [&self.geoip.country_db, &self.geoip.asn_db]
            .iter()
            .filter(|p| !p.exists())
            .map(|p| {
                format!(
                    "GeoIP database {} not found — run `tmon geoip-update`",
                    p.display()
                )
            })
            .collect()
    }
}

const SECTIONS: [&str; 9] = [
    "data_dir",
    "http",
    "collector",
    "storage",
    "retention",
    "geoip",
    "router",
    "capture",
    "dpi",
];

/// Only `TMON_DATA_DIR` and `TMON_<SECTION>__<KEY>` are config; other `TMON_*` vars are ignored.
fn is_config_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    SECTIONS
        .iter()
        .any(|s| k == *s || k.starts_with(&format!("{s}__")))
}

fn invalid(key: &str, value: &str) -> ConfigError {
    ConfigError::Invalid(format!("{key}: invalid address `{value}`"))
}

fn parse_addr(key: &str, v: &str) -> Result<SocketAddr, ConfigError> {
    v.parse().map_err(|_| invalid(key, v))
}

fn parse_all(key: &str, v: &[String]) -> Result<Vec<SocketAddr>, ConfigError> {
    v.iter().map(|s| parse_addr(key, s)).collect()
}

fn ensure_writable(dir: &Path) -> Result<(), ConfigError> {
    let err = |e: std::io::Error| {
        ConfigError::Invalid(format!("data_dir {} is not writable: {e}", dir.display()))
    };
    std::fs::create_dir_all(dir).map_err(err)?;
    let probe = dir.join(".tmon-write-test");
    std::fs::write(&probe, b"ok").map_err(err)?;
    std::fs::remove_file(&probe).map_err(err)
}
