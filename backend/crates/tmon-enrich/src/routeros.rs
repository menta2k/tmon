//! RouterOS v7 REST client (read-only): DHCP leases and DNS cache (FR-013).

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::Duration;
use tmon_store::duck::repo::settings::Router;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest router response accepted (lease lists and DNS caches are far smaller).
pub(crate) const MAX_BODY: usize = 4 * 1024 * 1024;
/// Router-supplied text kept in error messages (shown on /status).
const MAX_ERROR_TEXT: usize = 200;
/// Hostnames longer than this are ignored (DNS max is 253).
const MAX_HOSTNAME: usize = 253;

/// Reported by `/status` → `enrichment.router_integration`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RouterStatus {
    pub enabled: bool,
    pub last_ok: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub ip: IpAddr,
    pub mac: Option<String>,
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsEntry {
    pub ip: IpAddr,
    pub name: String,
}

#[derive(Deserialize)]
struct RawLease {
    address: Option<String>,
    #[serde(rename = "mac-address")]
    mac_address: Option<String>,
    #[serde(rename = "host-name")]
    host_name: Option<String>,
}

#[derive(Deserialize)]
struct RawDns {
    name: Option<String>,
    data: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("router rejected the credentials (HTTP 401)")]
    Unauthorized,
    #[error("router denied access: {0}")]
    Denied(String),
    #[error("router returned HTTP {0}")]
    Status(u16),
    #[error("router unreachable: {0}")]
    Transport(String),
    #[error("unexpected response from router: {0}")]
    Decode(String),
    #[error("invalid router settings: {0}")]
    Config(String),
}

pub(crate) fn clean_host(s: &str) -> Option<String> {
    let h = s.trim().trim_end_matches('.').to_ascii_lowercase();
    let ok = !h.is_empty()
        && h.len() <= MAX_HOSTNAME
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c));
    ok.then_some(h)
}

pub fn parse_leases(v: &[u8]) -> Result<Vec<Lease>, RouterError> {
    let raw: Vec<RawLease> =
        serde_json::from_slice(v).map_err(|e| RouterError::Decode(e.to_string()))?;
    Ok(raw
        .into_iter()
        .filter_map(|l| {
            let ip = l.address?.parse().ok()?;
            Some(Lease {
                ip,
                mac: l
                    .mac_address
                    .and_then(|m| tmon_core::rules::normalize_mac(&m)),
                hostname: l.host_name.as_deref().and_then(clean_host),
            })
        })
        .collect())
}

pub fn parse_dns(v: &[u8]) -> Result<Vec<DnsEntry>, RouterError> {
    let raw: Vec<RawDns> =
        serde_json::from_slice(v).map_err(|e| RouterError::Decode(e.to_string()))?;
    Ok(raw
        .into_iter()
        .filter(|d| matches!(d.kind.as_deref(), Some("A" | "AAAA")))
        .filter_map(|d| {
            Some(DnsEntry {
                ip: d.data?.parse().ok()?,
                name: clean_host(&d.name?)?,
            })
        })
        .collect())
}

/// Validated base URL without a trailing slash.
pub(crate) fn base_url(cfg: &Router) -> Result<String, RouterError> {
    let base = cfg.url.trim_end_matches('/').to_string();
    if !(base.starts_with("https://") || base.starts_with("http://")) {
        return Err(RouterError::Config(
            "url must start with http:// or https://".into(),
        ));
    }
    Ok(base)
}

/// HTTP client with the request timeout and the TLS policy from settings.
///
/// Redirects are not followed: a 307/308 would replay a login body (with the
/// password) to whatever host the redirect names.
pub(crate) fn http_client(cfg: &Router) -> Result<reqwest::Client, RouterError> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(!cfg.verify_tls)
        .build()
        .map_err(|e| RouterError::Config(e.to_string()))
}

/// Response body, refusing anything over [`MAX_BODY`].
pub(crate) async fn read_capped(mut res: reqwest::Response) -> Result<Vec<u8>, RouterError> {
    let too_large = || RouterError::Decode(format!("response too large (> {MAX_BODY} bytes)"));
    if res.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        return Err(too_large());
    }
    let mut out = Vec::new();
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|e| RouterError::Transport(e.without_url().to_string()))?
    {
        if out.len() + chunk.len() > MAX_BODY {
            return Err(too_large());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Router-supplied text, shortened for error messages.
pub(crate) fn snippet(text: &str) -> String {
    match text.char_indices().nth(MAX_ERROR_TEXT) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

/// `scheme://host[:port]` only, for logs (never userinfo or path).
pub fn display_target(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            let host = u.host_str()?.to_string();
            Some(match u.port() {
                Some(p) => format!("{}://{host}:{p}", u.scheme()),
                None => format!("{}://{host}", u.scheme()),
            })
        })
        .unwrap_or_else(|| "<invalid url>".into())
}

pub struct RouterClient {
    http: reqwest::Client,
    base: String,
    user: String,
    pass: String,
}

impl RouterClient {
    pub fn new(cfg: &Router) -> Result<Self, RouterError> {
        let base = base_url(cfg)?;
        Ok(Self {
            http: http_client(cfg)?,
            base,
            user: cfg.username.clone(),
            pass: cfg.password.clone(),
        })
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>, RouterError> {
        let res = self
            .http
            .get(format!("{}{path}", self.base))
            .basic_auth(&self.user, Some(&self.pass))
            .send()
            .await
            .map_err(|e| RouterError::Transport(e.without_url().to_string()))?;
        match res.status().as_u16() {
            200 => read_capped(res).await,
            401 => Err(RouterError::Unauthorized),
            s => Err(RouterError::Status(s)),
        }
    }

    pub async fn leases(&self) -> Result<Vec<Lease>, RouterError> {
        parse_leases(&self.get("/rest/ip/dhcp-server/lease").await?)
    }

    pub async fn dns_cache(&self) -> Result<Vec<DnsEntry>, RouterError> {
        parse_dns(&self.get("/rest/ip/dns/cache").await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_leases_and_dns() {
        let l = parse_leases(br#"[{"address":"192.168.88.10","mac-address":"AA:BB:CC:DD:EE:FF","host-name":"Laptop"},{"address":"x"},{"mac-address":"aa:bb:cc:dd:ee:00"}]"#).unwrap();
        assert_eq!(
            l,
            vec![Lease {
                ip: "192.168.88.10".parse().unwrap(),
                mac: Some("aa:bb:cc:dd:ee:ff".into()),
                hostname: Some("laptop".into())
            }]
        );
        let d = parse_dns(br#"[{"name":"video.example.com","data":"203.0.113.7","type":"A"},{"name":"x.com","data":"y.com","type":"CNAME"},{"name":"v6.example.com.","data":"2001:db8::7","type":"AAAA"},{"name":"bad name","data":"1.2.3.4","type":"A"}]"#).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[1].name, "v6.example.com");
        assert!(parse_leases(b"{}").is_err());
    }
}
