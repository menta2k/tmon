//! OpenWrt rpcd/ubus JSON-RPC client (read-only): DHCP leases.
//!
//! `POST /ubus` with `session.login`, then `luci-rpc.getDHCPLeases` using the
//! session token. The token is cached and renewed once when rpcd reports it
//! expired (JSON-RPC error -32002). OpenWrt has no DNS-cache API.

use serde::Deserialize;
use serde_json::{Value, json};
use tmon_store::duck::repo::settings::Router;
use tokio::sync::Mutex;

use crate::routeros::{
    Lease, RouterError, base_url, clean_host, http_client, read_capped, snippet,
};

/// Token rpcd expects for calls made before login.
const ANON_SESSION: &str = "00000000000000000000000000000000";
/// rpcd/ubus status codes in `result[0]`.
const UBUS_OK: u64 = 0;
const UBUS_PERMISSION_DENIED: u64 = 6;
/// JSON-RPC error rpcd returns for an unknown or expired session.
const RPC_ACCESS_DENIED: i64 = -32002;
/// Leases kept per poll (a home or office network has far fewer).
const MAX_LEASES: usize = 10_000;

pub struct OpenWrtClient {
    http: reqwest::Client,
    endpoint: String,
    user: String,
    pass: String,
    session: Mutex<Option<String>>,
}

/// Outcome of one ubus call.
enum Reply {
    Ok(Value),
    SessionExpired,
}

/// `Option` so that an explicit `null` list reads as empty.
#[derive(Deserialize)]
struct RawLeases {
    dhcp_leases: Option<Vec<RawV4>>,
    dhcp6_leases: Option<Vec<RawV6>>,
}

#[derive(Deserialize)]
struct RawV4 {
    ipaddr: Option<String>,
    macaddr: Option<String>,
    hostname: Option<String>,
}

#[derive(Deserialize)]
struct RawV6 {
    ip6addr: Option<String>,
    #[serde(default)]
    ip6addrs: Vec<String>,
    macaddr: Option<String>,
    hostname: Option<String>,
}

fn lease(ip: Option<&str>, mac: Option<&str>, host: Option<&str>) -> Option<Lease> {
    // DHCPv6 addresses come with a prefix length (`fd00::20/128`).
    let ip = ip?.split('/').next()?.parse().ok()?;
    Some(Lease {
        ip,
        mac: mac.and_then(tmon_core::rules::normalize_mac),
        hostname: host.and_then(clean_host),
    })
}

/// `getDHCPLeases` result → leases (v4 then v6; unparsable entries skipped).
pub fn parse_leases(v: Value) -> Result<Vec<Lease>, RouterError> {
    let raw: RawLeases =
        serde_json::from_value(v).map_err(|e| RouterError::Decode(e.to_string()))?;
    let (v4, v6) = (
        raw.dhcp_leases.unwrap_or_default(),
        raw.dhcp6_leases.unwrap_or_default(),
    );
    let v4 = v4.iter().filter_map(|l| {
        lease(
            l.ipaddr.as_deref(),
            l.macaddr.as_deref(),
            l.hostname.as_deref(),
        )
    });
    let v6 = v6.iter().filter_map(|l| {
        let ip = l
            .ip6addr
            .as_deref()
            .or(l.ip6addrs.first().map(String::as_str));
        lease(ip, l.macaddr.as_deref(), l.hostname.as_deref())
    });
    Ok(v4.chain(v6).take(MAX_LEASES).collect())
}

/// Decode a JSON-RPC envelope into the ubus payload (`result[1]`).
fn decode(body: &[u8], what: &str) -> Result<Reply, RouterError> {
    let v: Value =
        serde_json::from_slice(body).map_err(|e| RouterError::Decode(snippet(&e.to_string())))?;
    if let Some(err) = v.get("error") {
        return match err.get("code").and_then(Value::as_i64) {
            Some(RPC_ACCESS_DENIED) => Ok(Reply::SessionExpired),
            _ => Err(RouterError::Decode(format!(
                "{what}: {}",
                snippet(&err.to_string())
            ))),
        };
    }
    let result = v
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| RouterError::Decode(format!("{what}: no result")))?;
    match result.first().and_then(Value::as_u64) {
        Some(UBUS_OK) => Ok(Reply::Ok(result.get(1).cloned().unwrap_or(Value::Null))),
        Some(UBUS_PERMISSION_DENIED) => Err(RouterError::Denied(format!(
            "{what} not permitted by the rpcd ACL"
        ))),
        code => Err(RouterError::Decode(format!("{what}: ubus status {code:?}"))),
    }
}

impl OpenWrtClient {
    pub fn new(cfg: &Router) -> Result<Self, RouterError> {
        Ok(Self {
            endpoint: format!("{}/ubus", base_url(cfg)?),
            http: http_client(cfg)?,
            user: cfg.username.clone(),
            pass: cfg.password.clone(),
            session: Mutex::new(None),
        })
    }

    async fn call(
        &self,
        token: &str,
        object: &str,
        method: &str,
        args: Value,
    ) -> Result<Reply, RouterError> {
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "call",
            "params": [token, object, method, args],
        });
        let res = self
            .http
            .post(&self.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| RouterError::Transport(e.without_url().to_string()))?;
        match res.status().as_u16() {
            200 => {}
            401 | 403 => return Err(RouterError::Unauthorized),
            s => return Err(RouterError::Status(s)),
        }
        decode(&read_capped(res).await?, &format!("{object}.{method}"))
    }

    async fn login(&self) -> Result<String, RouterError> {
        let args = json!({"username": self.user, "password": self.pass});
        match self.call(ANON_SESSION, "session", "login", args).await {
            Ok(Reply::Ok(v)) => v
                .get("ubus_rpc_session")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| RouterError::Decode("session.login: no token".into())),
            // rpcd answers a wrong user/password with "permission denied".
            Ok(Reply::SessionExpired) | Err(RouterError::Denied(_)) => {
                Err(RouterError::Unauthorized)
            }
            Err(e) => Err(e),
        }
    }

    /// Call with the cached session, logging in (again) when needed.
    async fn authed(&self, object: &str, method: &str) -> Result<Value, RouterError> {
        let mut session = self.session.lock().await;
        for _ in 0..2 {
            let token = match session.as_ref() {
                Some(t) => t.clone(),
                None => session.insert(self.login().await?).clone(),
            };
            match self.call(&token, object, method, json!({})).await? {
                Reply::Ok(v) => return Ok(v),
                Reply::SessionExpired => *session = None,
            }
        }
        Err(RouterError::Unauthorized)
    }

    pub async fn leases(&self) -> Result<Vec<crate::routeros::Lease>, RouterError> {
        parse_leases(self.authed("luci-rpc", "getDHCPLeases").await?)
    }
}
