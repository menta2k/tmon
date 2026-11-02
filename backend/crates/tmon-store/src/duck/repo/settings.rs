//! UI-editable settings (data-model.md §9): file/env bootstrap, DB values win once set.

use duckdb::{Connection, OptionalExt, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tmon_core::ValidationErrors;

use crate::{StoreError, StoreResult};

const KEY: &str = "settings";
const MAX_DAYS: u32 = 3650;
const POLL_MIN: u64 = 5;
const POLL_MAX: u64 = 3600;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retention {
    pub raw_days: u32,
    pub rollup_1m_days: u32,
    pub rollup_1h_days: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub disk_cap_gb: f64,
}

/// Which router API the integration talks to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouterKind {
    /// MikroTik RouterOS v7 REST (`/rest/...`, HTTP Basic).
    #[default]
    #[serde(rename = "routeros")]
    RouterOs,
    /// OpenWrt rpcd/ubus JSON-RPC (`/ubus`, session login).
    #[serde(rename = "openwrt")]
    OpenWrt,
}

impl RouterKind {
    pub const ALL: [&'static str; 2] = ["routeros", "openwrt"];

    /// OpenWrt (dnsmasq) exposes no DNS cache; names come from capture instead.
    pub fn supports_dns_cache(self) -> bool {
        matches!(self, Self::RouterOs)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Router {
    /// Absent in settings saved before router kinds existed → RouterOS.
    #[serde(default)]
    pub kind: RouterKind,
    pub enabled: bool,
    pub url: String,
    pub username: String,
    /// Write-only: never serialized in API views (see [`Settings::view`]).
    #[serde(default)]
    pub password: String,
    pub verify_tls: bool,
    pub lease_poll_secs: u64,
    pub dns_poll_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub retention: Retention,
    pub storage: Storage,
    pub router: Router,
}

impl Settings {
    /// Public view: password removed, `password_set` added.
    pub fn view(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(r) = v.get_mut("router").and_then(Value::as_object_mut) {
            r.remove("password");
            r.insert(
                "password_set".into(),
                Value::Bool(!self.router.password.is_empty()),
            );
        }
        v
    }

    pub fn validate(&self) -> Result<(), ValidationErrors> {
        let mut e = ValidationErrors::new();
        let r = &self.retention;
        for (k, v) in [
            ("raw_days", r.raw_days),
            ("rollup_1m_days", r.rollup_1m_days),
            ("rollup_1h_days", r.rollup_1h_days),
        ] {
            if !(1..=MAX_DAYS).contains(&v) {
                e.add(format!("retention.{k}"), format!("must be 1–{MAX_DAYS}"));
            }
        }
        if !(self.storage.disk_cap_gb > 0.0 && self.storage.disk_cap_gb <= 1_000_000.0) {
            e.add("storage.disk_cap_gb", "must be > 0");
        }
        let rt = &self.router;
        if rt.enabled || !rt.url.is_empty() {
            let ok = (rt.url.starts_with("https://") || rt.url.starts_with("http://"))
                && rt.url.len() > 8
                && !rt.url.contains(char::is_whitespace);
            if !ok {
                e.add("router.url", "must be an http(s) URL");
            }
        }
        if rt.enabled && rt.username.trim().is_empty() {
            e.add("router.username", "required when enabled");
        }
        for (k, v) in [
            ("lease_poll_secs", rt.lease_poll_secs),
            ("dns_poll_secs", rt.dns_poll_secs),
        ] {
            if !(POLL_MIN..=POLL_MAX).contains(&v) {
                e.add(
                    format!("router.{k}"),
                    format!("must be {POLL_MIN}–{POLL_MAX}"),
                );
            }
        }
        e.into_result(())
    }

    /// Apply a deep-partial JSON patch; `router.password` is kept unless provided.
    pub fn patched(&self, patch: &Value) -> Result<Settings, ValidationErrors> {
        if !patch.is_object() {
            return Err(ValidationErrors::single("body", "must be an object"));
        }
        let mut base = serde_json::to_value(self)
            .map_err(|e| ValidationErrors::single("body", e.to_string()))?;
        merge(&mut base, patch);
        check_kind(&base)?;
        let next: Settings = serde_json::from_value(base)
            .map_err(|e| ValidationErrors::single("body", e.to_string()))?;
        next.validate()?;
        Ok(next)
    }
}

/// Field-level error for an unknown `router.kind` (serde would only say "body").
fn check_kind(merged: &Value) -> Result<(), ValidationErrors> {
    match merged.pointer("/router/kind") {
        None => Ok(()),
        Some(Value::String(k)) if RouterKind::ALL.contains(&k.as_str()) => Ok(()),
        Some(_) => Err(ValidationErrors::single(
            "router.kind",
            format!("must be one of {}", RouterKind::ALL.join("|")),
        )),
    }
}

/// RFC 7396-style merge of `patch` into `target` (null removes → keeps default via serde).
fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                match t.get_mut(k) {
                    Some(slot) if v.is_object() => merge(slot, v),
                    _ => {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

/// Stored settings, if any were saved from the UI.
pub fn load(conn: &Connection) -> StoreResult<Option<Settings>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM setting WHERE key = ?",
            params![KEY],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|s| {
        serde_json::from_str(&s)
            .map_err(|e| StoreError::Task(format!("stored settings unreadable: {e}")))
    })
    .transpose()
}

/// Effective settings: stored values win over the file/env defaults.
pub fn effective(conn: &Connection, defaults: &Settings) -> StoreResult<Settings> {
    Ok(load(conn)?.unwrap_or_else(|| defaults.clone()))
}

pub fn save(conn: &Connection, s: &Settings) -> StoreResult<()> {
    s.validate()?;
    let json = serde_json::to_string(s).map_err(|e| StoreError::Task(e.to_string()))?;
    conn.execute(
        "INSERT INTO setting VALUES (?, ?) ON CONFLICT DO UPDATE SET value = EXCLUDED.value",
        params![KEY, json],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Not;

    pub fn sample() -> Settings {
        Settings {
            retention: Retention {
                raw_days: 7,
                rollup_1m_days: 30,
                rollup_1h_days: 365,
            },
            storage: Storage { disk_cap_gb: 20.0 },
            router: Router {
                kind: RouterKind::default(),
                enabled: false,
                url: "https://192.168.88.1".into(),
                username: "tmon-ro".into(),
                password: "secret".into(),
                verify_tls: true,
                lease_poll_secs: 60,
                dns_poll_secs: 30,
            },
        }
    }

    #[test]
    fn view_hides_password() {
        let v = sample().view();
        assert!(v["router"].get("password").is_none());
        assert_eq!(v["router"]["password_set"], true);
        assert!(!v.to_string().contains("secret"));
    }

    #[test]
    fn deep_patch_keeps_password_and_validates() {
        let s = sample();
        let next = s
            .patched(
                &serde_json::json!({"retention": {"raw_days": 3}, "router": {"enabled": true}}),
            )
            .unwrap();
        assert_eq!(next.retention.raw_days, 3);
        assert_eq!(next.retention.rollup_1m_days, 30);
        assert_eq!(next.router.password, "secret");
        let err = s
            .patched(
                &serde_json::json!({"retention": {"raw_days": 0}, "router": {"url": "ftp://x"}}),
            )
            .unwrap_err();
        assert!(err.fields().contains_key("retention.raw_days"));
        assert!(err.fields().contains_key("router.url"));
        assert!(s.patched(&serde_json::json!({"bogus": 1})).is_err());
    }

    #[test]
    fn router_kind_defaults_to_routeros_and_accepts_openwrt() {
        let s = sample();
        assert_eq!(s.router.kind, RouterKind::RouterOs);
        // Settings saved before `kind` existed still load as RouterOS.
        let mut old = serde_json::to_value(&s).unwrap();
        old["router"].as_object_mut().unwrap().remove("kind");
        let loaded: Settings = serde_json::from_value(old).unwrap();
        assert_eq!(loaded.router.kind, RouterKind::RouterOs);
        let next = s
            .patched(&serde_json::json!({"router": {"kind": "openwrt"}}))
            .unwrap();
        assert_eq!(next.router.kind, RouterKind::OpenWrt);
        assert_eq!(next.view()["router"]["kind"], "openwrt");
        assert!(RouterKind::OpenWrt.supports_dns_cache().not());
        assert!(RouterKind::RouterOs.supports_dns_cache());
        let err = s
            .patched(&serde_json::json!({"router": {"kind": "pfsense"}}))
            .unwrap_err();
        assert!(err.fields().contains_key("router.kind"), "{err:?}");
    }

    #[test]
    fn db_values_win() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::DuckStore::open(dir.path(), &Default::default()).unwrap();
        let conn = store.conn().unwrap();
        let defaults = sample();
        assert_eq!(effective(&conn, &defaults).unwrap(), defaults);
        let changed = defaults
            .patched(&serde_json::json!({"storage": {"disk_cap_gb": 5.0}}))
            .unwrap();
        save(&conn, &changed).unwrap();
        assert_eq!(
            effective(&conn, &defaults).unwrap().storage.disk_cap_gb,
            5.0
        );
    }
}
