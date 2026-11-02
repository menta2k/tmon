//! Categories, rules, rule test, re-apply and jobs (FR-010..FR-012, FR-023).

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tmon_core::rules::{Rule, RuleMatch};
use tmon_core::{Categorizer, Category, Direction, FlowRecord, ValidationErrors};
use tmon_store::duck::repo::categories::{self, CategoryPatch};
use tmon_store::duck::repo::rules::{self, RuleInput, RuleView};

use crate::envelope::{ApiError, ApiResult, ok};
use crate::jobs::Job;
use crate::params::parse_time;
use crate::state::AppState;

pub const HITS_WINDOW_MS: i64 = 86_400_000;
/// Short TTL: the count scans 24 h of raw data, but must reflect fresh replays quickly.
pub const HITS_CACHE_MS: i64 = 15_000;

type Body = Result<Json<Value>, axum::extract::rejection::JsonRejection>;

fn body<T: serde::de::DeserializeOwned>(b: Body) -> Result<T, ApiError> {
    let Json(v) = b.map_err(|e| ApiError::validation(e.body_text()))?;
    serde_json::from_value(v).map_err(|e| ApiError::validation(e.to_string()))
}

fn json_body(b: Body) -> Result<Value, ApiError> {
    b.map(|Json(v)| v)
        .map_err(|e| ApiError::validation(e.body_text()))
}

/// Recompile the live rule set after a mutation.
async fn reload(s: &AppState) -> Result<(), ApiError> {
    let p = s.pipeline.clone();
    s.blocking(move |st| tmon_enrich::ruleset_handle::reload(st, &p).map(|_| ()))
        .await?;
    *s.hits_cache
        .lock()
        .map_err(|_| ApiError::Internal("hits cache poisoned".into()))? = None;
    Ok(())
}

// ---- categories ----

pub async fn list_categories(State(s): State<AppState>) -> ApiResult<Vec<Category>> {
    ok(s.blocking(|st| categories::list(&st.conn()?)).await?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewCategory {
    name: String,
    color: String,
    #[serde(default)]
    description: String,
}

pub async fn create_category(State(s): State<AppState>, b: Body) -> ApiResult<Category> {
    let n: NewCategory = body(b)?;
    ok(s.blocking(move |st| {
        st.with_writer(|c| categories::create(c, &n.name, &n.color, &n.description))
    })
    .await?)
}

pub async fn update_category(
    State(s): State<AppState>,
    Path(id): Path<u16>,
    b: Body,
) -> ApiResult<Category> {
    let p: CategoryPatch = body(b)?;
    ok(
        s.blocking(move |st| st.with_writer(|c| categories::update(c, id, &p)))
            .await?,
    )
}

pub async fn delete_category(State(s): State<AppState>, Path(id): Path<u16>) -> ApiResult<()> {
    s.blocking(move |st| st.with_writer(|c| categories::delete(c, id)))
        .await?;
    ok(())
}

// ---- rules ----

async fn hits(s: &AppState) -> Result<HashMap<u32, u64>, ApiError> {
    let now = s.now_ms();
    if let Some((at, h)) = s
        .hits_cache
        .lock()
        .map_err(|_| ApiError::Internal("hits cache poisoned".into()))?
        .as_ref()
        && now - at < HITS_CACHE_MS
    {
        return Ok(h.as_ref().clone());
    }
    let h = s
        .blocking(move |st| rules::hits(&st.conn()?, st.raw_dir(), now - HITS_WINDOW_MS, now + 1))
        .await?;
    *s.hits_cache
        .lock()
        .map_err(|_| ApiError::Internal("hits cache poisoned".into()))? =
        Some((now, Arc::new(h.clone())));
    Ok(h)
}

pub async fn list_rules(State(s): State<AppState>) -> ApiResult<Vec<RuleView>> {
    let all = s.blocking(|st| rules::list(&st.conn()?)).await?;
    let h = hits(&s).await?;
    ok(all
        .into_iter()
        .map(|rule| RuleView {
            hits_24h: h.get(&rule.id).copied().unwrap_or(0),
            rule,
        })
        .collect())
}

/// Parse `{name, category_id, enabled, match}` (each optional for PATCH).
fn rule_fields(v: &Value, cur: Option<&Rule>) -> Result<RuleInput, ApiError> {
    let obj = v
        .as_object()
        .ok_or_else(|| ApiError::validation("body must be an object"))?;
    let mut e = ValidationErrors::new();
    for k in obj.keys() {
        if !["name", "category_id", "enabled", "match"].contains(&k.as_str()) {
            e.add(k.clone(), "unknown field");
        }
    }
    let name = match obj.get("name") {
        Some(Value::String(n)) => n.clone(),
        Some(_) => {
            e.add("name", "must be a string");
            String::new()
        }
        None => cur.map(|r| r.name.clone()).unwrap_or_else(|| {
            e.add("name", "required");
            String::new()
        }),
    };
    let category_id = match obj.get("category_id") {
        Some(v) => v
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or_else(|| {
                e.add("category_id", "must be a category id");
                0
            }),
        None => cur.map(|r| r.category_id).unwrap_or_else(|| {
            e.add("category_id", "required");
            0
        }),
    };
    let enabled = match obj.get("enabled") {
        Some(v) => v.as_bool().unwrap_or_else(|| {
            e.add("enabled", "must be a boolean");
            true
        }),
        None => cur.is_none_or(|r| r.enabled),
    };
    let match_ = match obj.get("match") {
        Some(m) => RuleMatch::from_json(m).unwrap_or_else(|err| {
            e.merge_prefixed("match", err);
            RuleMatch::default()
        }),
        None => cur.map(|r| r.match_.clone()).unwrap_or_else(|| {
            e.add("match", "required");
            RuleMatch::default()
        }),
    };
    Ok(e.into_result(RuleInput {
        name,
        category_id,
        enabled,
        match_,
    })?)
}

pub async fn create_rule(State(s): State<AppState>, b: Body) -> ApiResult<Rule> {
    let input = rule_fields(&json_body(b)?, None)?;
    let rule = s
        .blocking(move |st| st.with_writer(|c| rules::create(c, input)))
        .await?;
    reload(&s).await?;
    ok(rule)
}

pub async fn update_rule(
    State(s): State<AppState>,
    Path(id): Path<u32>,
    b: Body,
) -> ApiResult<Rule> {
    let v = json_body(b)?;
    let cur = s.blocking(move |st| rules::get(&st.conn()?, id)).await?;
    let input = rule_fields(&v, Some(&cur))?;
    let rule = s
        .blocking(move |st| {
            st.with_writer(|c| {
                rules::update(c, id, |r| Rule {
                    name: input.name,
                    category_id: input.category_id,
                    enabled: input.enabled,
                    match_: input.match_,
                    ..r.clone()
                })
            })
        })
        .await?;
    reload(&s).await?;
    ok(rule)
}

pub async fn delete_rule(State(s): State<AppState>, Path(id): Path<u32>) -> ApiResult<()> {
    s.blocking(move |st| st.with_writer(|c| rules::delete(c, id)))
        .await?;
    reload(&s).await?;
    ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reorder {
    ids: Vec<u32>,
}

pub async fn reorder(State(s): State<AppState>, b: Body) -> ApiResult<()> {
    let r: Reorder = body(b)?;
    s.blocking(move |st| st.with_writer(|c| rules::reorder(c, &r.ids)))
        .await?;
    reload(&s).await?;
    ok(())
}

// ---- rule test ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestFlow {
    proto: Option<u8>,
    local_ip: Option<String>,
    local_port: Option<u16>,
    remote_ip: Option<String>,
    remote_port: Option<u16>,
    country: Option<String>,
    asn: Option<u32>,
    remote_host: Option<String>,
    direction: Option<String>,
    /// Classifier application (feature 003).
    app: Option<String>,
    app_category: Option<String>,
    app_confident: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestBody {
    flow: TestFlow,
}

fn test_record(t: TestFlow) -> Result<FlowRecord, ApiError> {
    let ip = |v: Option<String>, k: &str, d: &str| {
        v.as_deref()
            .unwrap_or(d)
            .parse::<std::net::IpAddr>()
            .map_err(|_| ApiError::field(k, "must be an IP address"))
    };
    let direction = match t.direction.as_deref() {
        None => Direction::Down,
        Some(d) => Direction::parse(d)
            .ok_or_else(|| ApiError::field("flow.direction", "must be down|up|internal|transit"))?,
    };
    let app = test_app(&t)?;
    let local = ip(t.local_ip, "flow.local_ip", "192.168.0.2")?;
    let remote = ip(t.remote_ip, "flow.remote_ip", "198.51.100.1")?;
    let raw = tmon_core::RawFlow {
        ts_ms: 0,
        ts_start_ms: 0,
        exporter_id: 0,
        in_if: 0,
        out_if: 0,
        src_ip: local,
        dst_ip: remote,
        src_port: t.local_port.unwrap_or(0),
        dst_port: t.remote_port.unwrap_or(0),
        proto: t.proto.unwrap_or(6),
        tcp_flags: 0,
        bytes: 0,
        packets: 0,
        sampled: false,
        nat_ip: None,
        nat_port: None,
        nat_dst_ip: None,
        nat_dst_port: None,
        flow_direction: None,
    };
    let rec = FlowRecord::from_raw(&raw, direction, true)
        .with_geo(
            t.country.map(|c| c.to_ascii_uppercase().into()),
            t.asn,
            None,
        )
        .with_remote_host(t.remote_host.map(|h| h.to_ascii_lowercase().into()));
    Ok(match app {
        Some(a) => rec.with_app(&a),
        None => rec,
    })
}

/// The test flow's application, validated like classifier output.
fn test_app(t: &TestFlow) -> Result<Option<tmon_core::AppInfo>, ApiError> {
    let Some(app) = t.app.as_deref() else {
        return match t.app_category {
            Some(_) => Err(ApiError::field("flow.app", "required with app_category")),
            None => Ok(None),
        };
    };
    let cat = t.app_category.as_deref().unwrap_or("");
    let confident = t.app_confident.unwrap_or(true);
    tmon_core::AppInfo::new(app, cat, confident, None, false)
        .map(Some)
        .map_err(|e| ApiError::field("flow.app", &e.to_string()))
}

pub async fn test_rule(State(s): State<AppState>, b: Body) -> ApiResult<Value> {
    let t: TestBody = body(b)?;
    let rec = test_record(t.flow)?;
    let rules = s.pipeline.rules();
    let (category_id, rule_id) = rules.categorize(&rec);
    let confidence = rules
        .find(&rec)
        .map_or(tmon_core::rules::Confidence::None, |r| {
            r.confidence_for(&rec)
        });
    ok(serde_json::json!({
        "category_id": category_id,
        "rule_id": rule_id,
        "confidence": confidence.as_str(),
    }))
}

// ---- re-apply ----

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Reapply {
    from: Option<String>,
    to: Option<String>,
}

pub async fn reapply(State(s): State<AppState>, b: Body) -> ApiResult<Value> {
    let r: Reapply = match json_body(b) {
        Ok(Value::Null) => Reapply::default(),
        Ok(v) => serde_json::from_value(v).map_err(|e| ApiError::validation(e.to_string()))?,
        Err(e) => return Err(e),
    };
    let now = s.now_ms();
    let from = match r.from {
        Some(f) => parse_time(&f, now)
            .ok_or_else(|| ApiError::field("from", "must be RFC3339 or now-N"))?,
        None => now - s.policy().raw_ms,
    };
    let to = match r.to {
        Some(t) => {
            parse_time(&t, now).ok_or_else(|| ApiError::field("to", "must be RFC3339 or now-N"))?
        }
        None => now + 1,
    };
    if from >= to {
        return Err(ApiError::field("from", "must be before `to`"));
    }
    let (store, rules) = (s.store.clone(), s.pipeline.rules());
    let id = s
        .jobs
        .submit(Box::new(move |progress| {
            tmon_store::duck::recategorize::recategorize(&store, from, to, rules.as_ref(), progress)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }))
        .map_err(ApiError::Internal)?;
    ok(serde_json::json!({"job_id": id}))
}

pub async fn job(State(s): State<AppState>, Path(id): Path<u64>) -> ApiResult<Job> {
    ok(s.jobs.get(id).ok_or(ApiError::NotFound)?)
}
