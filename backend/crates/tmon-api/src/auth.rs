//! Single-user authentication: argon2 password, in-memory sessions, bearer
//! token, and a per-IP login rate limit (FR-024).

use argon2::password_hash::PasswordHash;
use argon2::{Argon2, PasswordVerifier};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, header};
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;

use crate::envelope::ApiError;
use crate::state::AppState;

pub const COOKIE: &str = "tmon_session";
pub const MAX_LOGINS_PER_MINUTE: usize = 5;
const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;

pub struct Auth {
    password_hash: String,
    api_token: Option<String>,
    ttl_ms: i64,
    sessions: Mutex<HashMap<String, i64>>,
    attempts: Mutex<HashMap<IpAddr, Vec<i64>>>,
}

impl Auth {
    pub fn new(password_hash: String, api_token: Option<String>, ttl_hours: u64) -> Self {
        Self {
            password_hash,
            api_token: api_token.filter(|t| !t.is_empty()),
            ttl_ms: i64::try_from(ttl_hours).unwrap_or(168) * HOUR_MS,
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(HashMap::new()),
        }
    }

    pub fn ttl_secs(&self) -> i64 {
        self.ttl_ms / 1000
    }

    /// Record a login attempt; `Err(RateLimited)` beyond 5/min/IP.
    pub fn check_rate(&self, ip: IpAddr, now_ms: i64) -> Result<(), ApiError> {
        let mut map = self
            .attempts
            .lock()
            .map_err(|_| ApiError::Internal("lock".into()))?;
        let recent: Vec<i64> = map
            .get(&ip)
            .into_iter()
            .flatten()
            .copied()
            .filter(|t| now_ms - t < MINUTE_MS)
            .collect();
        if recent.len() >= MAX_LOGINS_PER_MINUTE {
            map.insert(ip, recent);
            return Err(ApiError::RateLimited);
        }
        let next: Vec<i64> = recent.into_iter().chain(std::iter::once(now_ms)).collect();
        map.insert(ip, next);
        map.retain(|_, v| v.iter().any(|t| now_ms - t < MINUTE_MS));
        Ok(())
    }

    pub fn verify_password(&self, password: &str) -> bool {
        PasswordHash::new(&self.password_hash)
            .map(|h| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &h)
                    .is_ok()
            })
            .unwrap_or(false)
    }

    /// Create a session and return its id.
    pub fn create_session(&self, now_ms: i64) -> Result<String, ApiError> {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let id = URL_SAFE_NO_PAD.encode(bytes);
        let mut s = self
            .sessions
            .lock()
            .map_err(|_| ApiError::Internal("lock".into()))?;
        s.retain(|_, exp| *exp > now_ms);
        s.insert(id.clone(), now_ms + self.ttl_ms);
        Ok(id)
    }

    pub fn end_session(&self, id: &str) {
        if let Ok(mut s) = self.sessions.lock() {
            s.remove(id);
        }
    }

    fn session_valid(&self, id: &str, now_ms: i64) -> bool {
        self.sessions
            .lock()
            .map(|s| s.get(id).is_some_and(|exp| *exp > now_ms))
            .unwrap_or(false)
    }

    /// True when the request carries a valid session cookie or bearer token.
    pub fn is_authorized(&self, headers: &HeaderMap, now_ms: i64) -> bool {
        if let (Some(token), Some(bearer)) = (&self.api_token, bearer(headers)) {
            if constant_eq(token.as_bytes(), bearer.as_bytes()) {
                return true;
            }
        }
        session_cookie(headers).is_some_and(|id| self.session_valid(&id, now_ms))
    }

    /// True only for a configured token presented as `Bearer`; with no token configured,
    /// `/metrics` requires a session (never anonymous).
    pub fn token_ok(&self, headers: &HeaderMap) -> bool {
        match (&self.api_token, bearer(headers)) {
            (Some(t), Some(b)) => constant_eq(t.as_bytes(), b.as_bytes()),
            _ => false,
        }
    }

    /// Forget an IP's attempts after a successful login, so only failures throttle.
    pub fn reset_rate(&self, ip: IpAddr) {
        if let Ok(mut map) = self.attempts.lock() {
            map.remove(&ip);
        }
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn bearer(h: &HeaderMap) -> Option<String> {
    h.get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.trim().to_string())
}

pub fn session_cookie(h: &HeaderMap) -> Option<String> {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            (k == COOKIE).then(|| v.to_string())
        })
}

pub fn set_cookie(id: &str, max_age_secs: i64) -> String {
    format!("{COOKIE}={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age_secs}")
}

pub fn client_ip(info: Option<&ConnectInfo<SocketAddr>>) -> IpAddr {
    info.map_or(IpAddr::from([127, 0, 0, 1]), |c| c.0.ip())
}

/// Middleware guarding protected routes.
pub async fn require_auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if state.auth.is_authorized(req.headers(), state.now_ms()) {
        Ok(next.run(req).await)
    } else {
        Err(ApiError::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_and_sessions() {
        let a = Auth::new("bad-hash".into(), Some("tok".into()), 1);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..5 {
            a.check_rate(ip, 1000).unwrap();
        }
        assert_eq!(a.check_rate(ip, 1000), Err(ApiError::RateLimited));
        a.check_rate(ip, 1000 + MINUTE_MS + 1).unwrap();
        assert!(!a.verify_password("x"));
        let id = a.create_session(0).unwrap();
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            format!("a=b; {COOKIE}={id}").parse().unwrap(),
        );
        assert!(a.is_authorized(&h, 10));
        assert!(!a.is_authorized(&h, HOUR_MS + 1));
        a.end_session(&id);
        assert!(!a.is_authorized(&h, 10));
        let mut b = HeaderMap::new();
        b.insert(header::AUTHORIZATION, "Bearer tok".parse().unwrap());
        assert!(a.is_authorized(&b, 0));
        assert!(a.token_ok(&b));
        assert!(!a.token_ok(&HeaderMap::new()));
        let open = Auth::new("bad-hash".into(), None, 1);
        assert!(!open.token_ok(&b), "no configured token never authorizes");
        a.reset_rate(ip);
        a.check_rate(ip, 1000).unwrap();
    }
}
