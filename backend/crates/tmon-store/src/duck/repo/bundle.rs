//! Settings export/import bundle (FR-026): categories, rules, hosts, networks, settings — no secrets.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tmon_core::rules::{Rule, RuleMatch};
use tmon_core::{Category, ValidationErrors};

use super::networks::{self, NetworkRow};
use super::settings::Settings;
use super::{categories, hosts, rules, settings};
use crate::{DuckStore, StoreResult};

pub const VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostEntry {
    pub ip: String,
    pub mac: Option<String>,
    pub friendly_name: Option<String>,
}

/// A fully validated bundle, ready to apply.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub categories: Vec<Category>,
    pub rules: Vec<Rule>,
    pub hosts: Vec<HostEntry>,
    pub networks: Vec<NetworkRow>,
    pub settings: Settings,
}

pub fn export(store: &DuckStore, current: &Settings) -> StoreResult<Value> {
    let c = store.conn()?;
    let (all_hosts, _) = hosts::list(&c, None, 1, u64::MAX / 2)?;
    let host_entries: Vec<HostEntry> = all_hosts
        .into_iter()
        .filter(|h| h.friendly_name.is_some() || h.mac.is_some())
        .map(|h| HostEntry {
            ip: h.ip,
            mac: h.mac,
            friendly_name: h.friendly_name,
        })
        .collect();
    Ok(json!({
        "version": VERSION,
        "categories": categories::list(&c)?,
        "rules": rules::list(&c)?,
        "hosts": host_entries,
        "networks": networks::list(&c)?,
        "settings": current.view(),
    }))
}

fn array<'a>(v: &'a Value, key: &str, e: &mut ValidationErrors) -> &'a [Value] {
    match v.get(key).and_then(Value::as_array) {
        Some(a) => a,
        None => {
            e.add(key, "must be an array");
            &[]
        }
    }
}

fn parse_categories(items: &[Value], e: &mut ValidationErrors) -> Vec<Category> {
    let mut out: Vec<Category> = Vec::new();
    for (i, v) in items.iter().enumerate() {
        match serde_json::from_value::<Category>(v.clone()) {
            Ok(c) => {
                if let Err(err) = c.validate() {
                    e.merge_prefixed(&format!("categories[{i}]"), err);
                }
                if out
                    .iter()
                    .any(|o| o.id == c.id || o.name.eq_ignore_ascii_case(&c.name))
                {
                    e.add(format!("categories[{i}]"), "duplicate id or name");
                }
                out.push(c);
            }
            Err(err) => e.add(format!("categories[{i}]"), err.to_string()),
        }
    }
    if !out.iter().any(|c| c.id == tmon_core::OTHER_ID) {
        e.add("categories", "must include the Other category (id 0)");
    }
    out
}

fn parse_rule(v: &Value, cats: &BTreeSet<u16>) -> Result<Rule, ValidationErrors> {
    let mut e = ValidationErrors::new();
    let m = v
        .get("match")
        .map(RuleMatch::from_json)
        .unwrap_or_else(|| Err(ValidationErrors::single("match", "required")));
    let mut stripped = v.clone();
    if let Some(o) = stripped.as_object_mut() {
        o.remove("match");
        o.remove("hits_24h");
        o.insert("match".into(), json!({}));
    }
    let base: Result<Rule, _> = serde_json::from_value(stripped);
    let (base, m) = match (base, m) {
        (Ok(b), Ok(m)) => (b, m),
        (b, m) => {
            if let Err(err) = b {
                e.add("rule", err.to_string());
            }
            if let Err(err) = m {
                e.merge_prefixed("match", err);
            }
            return Err(e);
        }
    };
    let rule = Rule { match_: m, ..base };
    rule.validate()?;
    if !cats.contains(&rule.category_id) {
        return Err(ValidationErrors::single("category_id", "unknown category"));
    }
    Ok(rule)
}

fn parse_hosts(items: &[Value], e: &mut ValidationErrors) -> Vec<HostEntry> {
    items
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let ip = v
                .get("ip")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<std::net::IpAddr>().ok());
            let mac = v.get("mac").and_then(Value::as_str);
            let friendly = v
                .get("friendly_name")
                .and_then(Value::as_str)
                .map(str::to_string);
            let Some(ip) = ip else {
                e.add(format!("hosts[{i}].ip"), "must be an IP address");
                return None;
            };
            let mac = match mac.map(tmon_core::rules::normalize_mac) {
                Some(None) => {
                    e.add(format!("hosts[{i}].mac"), "invalid MAC address");
                    None
                }
                Some(m) => m,
                None => None,
            };
            if friendly.as_ref().is_some_and(|f| f.chars().count() > 64) {
                e.add(
                    format!("hosts[{i}].friendly_name"),
                    "must be at most 64 characters",
                );
            }
            Some(HostEntry {
                ip: ip.to_string(),
                mac,
                friendly_name: friendly,
            })
        })
        .collect()
}

/// Validate a whole bundle; nothing is applied when any item is invalid.
pub fn validate(v: &Value, current: &Settings) -> Result<Bundle, ValidationErrors> {
    let mut e = ValidationErrors::new();
    if v.get("version").and_then(Value::as_u64) != Some(VERSION) {
        e.add("version", format!("must be {VERSION}"));
    }
    let cats = parse_categories(array(v, "categories", &mut e), &mut e);
    let ids: BTreeSet<u16> = cats.iter().map(|c| c.id).collect();
    let mut rule_list = Vec::new();
    for (i, r) in array(v, "rules", &mut e).iter().enumerate() {
        match parse_rule(r, &ids) {
            Ok(rule) if rule_list.iter().any(|o: &Rule| o.id == rule.id) => {
                e.add(format!("rules[{i}].id"), "duplicate id")
            }
            Ok(rule) => rule_list.push(rule),
            Err(err) => e.merge_prefixed(&format!("rules[{i}]"), err),
        }
    }
    let host_list = parse_hosts(array(v, "hosts", &mut e), &mut e);
    let nets: Vec<NetworkRow> = serde_json::from_value(
        v.get("networks").cloned().unwrap_or(Value::Null),
    )
    .unwrap_or_else(|err| {
        e.add("networks", err.to_string());
        Vec::new()
    });
    if let Err(err) = networks::validate(&nets) {
        for (k, msg) in err.fields() {
            e.add(format!("networks{k}"), msg.clone());
        }
    }
    let mut settings_patch = v.get("settings").cloned().unwrap_or_else(|| json!({}));
    if let Some(r) = settings_patch
        .get_mut("router")
        .and_then(Value::as_object_mut)
    {
        r.remove("password_set");
        r.remove("password");
    }
    let new_settings = current.patched(&settings_patch).unwrap_or_else(|err| {
        e.merge_prefixed("settings", err);
        current.clone()
    });
    e.into_result(Bundle {
        categories: cats,
        rules: rule_list,
        hosts: host_list,
        networks: nets,
        settings: new_settings,
    })
}

/// Apply a validated bundle in one transaction.
pub fn apply(store: &DuckStore, b: &Bundle, now_ms: i64) -> StoreResult<()> {
    store.with_writer(|c| {
        let tx = c.unchecked_transaction()?;
        rules::replace_all(&tx, &[])?;
        categories::replace_all(&tx, &b.categories)?;
        rules::replace_all(&tx, &b.rules)?;
        networks::write_rows(&tx, &b.networks)?;
        let host_rows: Vec<_> = b
            .hosts
            .iter()
            .map(|h| (h.ip.clone(), h.mac.clone(), h.friendly_name.clone()))
            .collect();
        hosts::import(&tx, &host_rows, now_ms)?;
        settings::save(&tx, &b.settings)?;
        tx.commit()?;
        Ok(())
    })
}
