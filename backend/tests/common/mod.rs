//! Integration harness: run the full app on ephemeral ports and replay fixtures.
#![allow(dead_code)]

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tmon::app::{self, Running};
use tmon::config::{CollectorConfig, Config, HttpConfig};
use tmon_collector::listener::ListenerKind;

pub const TOKEN: &str = "it-token";
/// argon2 hash of "changeme".
pub const PASSWORD: &str = "changeme";

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn hash(pw: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt = SaltString::from_b64("c2FsdHNhbHRzYWx0c2FsdA").unwrap();
    argon2::Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

pub struct It {
    pub dir: tempfile::TempDir,
    pub app: Running,
    pub http: reqwest::Client,
}

pub async fn start() -> It {
    start_with(|_| {}).await
}

/// Start with a config tweak (e.g. enabling capture).
pub async fn start_with(tweak: impl FnOnce(&mut Config)) -> It {
    let dir = tempfile::tempdir().unwrap();
    let base = Config::default();
    let config = Config {
        data_dir: dir.path().join("data"),
        http: HttpConfig {
            listen: "127.0.0.1:0".into(),
            password_hash: hash(PASSWORD),
            api_token: TOKEN.into(),
            ..base.http.clone()
        },
        collector: CollectorConfig {
            netflow_listen: vec!["127.0.0.1:0".into()],
            sflow_listen: vec!["127.0.0.1:0".into()],
            ..base.collector.clone()
        },
        ..base
    };
    let mut config = config;
    tweak(&mut config);
    std::fs::create_dir_all(&config.data_dir).unwrap();
    let app = app::start(config.validate().unwrap()).await.unwrap();
    It {
        dir,
        app,
        http: reqwest::Client::new(),
    }
}

impl It {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/api/v1{path}", self.app.http_addr)
    }

    pub async fn get(&self, path: &str) -> (u16, Value) {
        let res = self
            .http
            .get(self.url(path))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        let status = res.status().as_u16();
        (
            status,
            serde_json::from_str(&res.text().await.unwrap()).unwrap_or(Value::Null),
        )
    }

    pub async fn send_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Value,
    ) -> (u16, Value) {
        let res = self
            .http
            .request(method, self.url(path))
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = res.status().as_u16();
        (
            status,
            serde_json::from_str(&res.text().await.unwrap()).unwrap_or(Value::Null),
        )
    }

    pub async fn replay(&self, name: &str, kind: ListenerKind) {
        let to = self.app.udp_addr(kind).expect("listener").to_string();
        tmon::cli::replay::run(&fixture(name), &to, "max")
            .await
            .unwrap();
    }

    /// Flush until the total bytes of `by=category` reach `min_total` (or time out).
    pub async fn wait_for_bytes(&self, query: &str, min_total: u64) -> Value {
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            self.app.flush().await.unwrap();
            let (_, body) = self.get(query).await;
            let total: u64 = body["data"]
                .as_array()
                .map(|a| a.iter().filter_map(|r| r["bytes"].as_u64()).sum())
                .unwrap_or(0);
            if total >= min_total {
                return body;
            }
        }
        let (_, last) = self.get(query).await;
        panic!("timed out waiting for {min_total} bytes at {query}: {last}");
    }
}

/// Parse a truth CSV (`category,direction,local_ip,bytes`) into bytes per `key_col`.
pub fn truth_by(name: &str, key_col: usize) -> BTreeMap<String, u64> {
    let text = std::fs::read_to_string(fixture(name)).unwrap();
    let mut out = BTreeMap::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split(',').collect();
        *out.entry(cols[key_col].to_string()).or_default() += cols[3].parse::<u64>().unwrap();
    }
    out
}

pub fn within(actual: u64, expected: u64, pct: f64) -> bool {
    (actual as f64 - expected as f64).abs() <= expected as f64 * pct / 100.0
}
