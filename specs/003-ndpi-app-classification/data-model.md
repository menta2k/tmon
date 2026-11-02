# Data Model: Application Classification (feature 003)

## 1. Core types (`tmon-core/src/app.rs`)

### AppInfo

| Field | Type | Rules |
|---|---|---|
| `app` | `Arc<str>` | Application name as reported (e.g. `BitTorrent`). 1–48 printable ASCII chars; anything else is rejected (`app_invalid` counter). Never a generic carrier: `TLS`, `QUIC`, `HTTP`, `DNS`, `Unknown` (see `GENERIC_CARRIERS`) |
| `category` | `Arc<str>` | Classifier category name (e.g. `VPN`), 1–48 printable ASCII; may be empty |
| `confident` | `bool` | true for nDPI confidence `DPI`, `DPI_CACHE` or `CUSTOM_RULE` (research R6) |
| `host` | `Option<Arc<str>>` | goes through `normalize_host` (feature 002); invalid → `None` |
| `final_` | `bool` | the classifier will not refine this connection further |

`AppInfo::better_than(&self, old) -> bool` returns true when `(confident, !generic)` is not lower and the app differs. This implements FR-006 (a guess never replaces a confident result).

### trait ConnectionClassifier

```rust
pub trait ConnectionClassifier: Send + Sync {
    fn classify(&self, key: &ConnKey, now_ms: i64) -> Option<Arc<AppInfo>>;
}
```

`ConnKey` is the feature 002 LAN-side 5-tuple.

### NameSource

Adds `Dpi`: code `6`, serialized `"dpi"`, label "DPI". Priority is below every feature 002 source.

## 2. FlowRecord additions (`tmon-core/src/flow.rs`)

| Field | Type | Persisted | Notes |
|---|---|---|---|
| `app` | `Option<Arc<str>>` | yes, `app VARCHAR` | |
| `app_confident` | `bool` | yes, `app_confident BOOLEAN` | false when `app` is None (stored NULL) |
| `app_category` | `Option<Arc<str>>` | **no** (`#[serde(skip)]`) | rule matching at ingest only, like `remote_aliases`. Re-apply to history matches `app` only (documented limitation) |

Builder: `with_app(info: &AppInfo)`, which returns a new record (no mutation).

**Enrichment order** (FR-005), after feature 002's naming step:

1. 002 name present → keep it.
2. Else, if `info.host` is set → `remote_host = host`, `remote_host_source = Dpi`.
3. Set `app`, `app_confident` and `app_category` from `info`.
4. Categorize.

## 3. Rules (`tmon-core/src/rules`)

`RuleMatch` gains:

| Field | Type | Validation |
|---|---|---|
| `app` | `Option<Vec<String>>` | 1–64 entries, each 1–48 printable ASCII, compared case-insensitively |
| `app_category` | `Option<Vec<String>>` | same |

- **Matching**: `app` matches `flow.app`; `app_category` matches `flow.app_category`. A flow with no app matches neither.
- **Confidence** becomes `RuleMatch::confidence_for(&self, flow) -> Confidence`:
  - `hostname` → Detected;
  - `app`/`app_category` matched with `flow.app_confident` → Detected;
  - `app`/`app_category` matched without it → Inferred;
  - the rest is unchanged.
- **Candidate index**: an app-name index is added beside the hostname index, so rule evaluation stays sub-µs (feature 001 bench).

**Built-in data** (`backend/assets/builtin-rules.toml`): new category `File Sharing` (id 15) and about 25 rules in priority bands `860_000–869_999` (apps) and `850_000–859_999` (nDPI categories), per research R10.

## 4. Sidecar side (`backend/dpi/tmon-dpi`)

### DpiFlow (one per oriented 5-tuple)

| Field | Type | Notes |
|---|---|---|
| `key` | `ConnKey` | |
| `ndpi` | owned `*mut ndpi_flow_struct` | freed with `ndpi_flow_free` on drop (RAII wrapper) |
| `packets` | `u16` | give up at `max_packets_per_conn` |
| `reported` | `Option<(app_id, confident)>` | send a Classification only on change (FR-006) |
| `last_seen_ms` | `i64` | idle timeout (default 15 min) |

**State**:

```text
new ──packet──► inspecting ──(classified, confident or giveup)──► final (Classification final=true, then dropped)
     inspecting ──(guess)──► inspecting (Classification final=false)
     any ──idle timeout / LRU / memory budget──► freed (counted)
```

**Bounded by**: `max_flows` (LRU), the allocator byte budget (research R9) and `RLIMIT_AS`.

### Counters (sidecar → `Stats` message)

`packets_in`, `packets_invalid`, `flows_active`, `flows_created`, `flows_evicted_{lru,idle,memory}`, `classified_confident`, `classified_guess`, `giveups`, `mem_bytes`, `mem_limit_bytes`, `top_apps[≤ 16] (name, flows)`.

## 5. tmon side (`backend/crates/tmon-dpi-client`)

### DpiSettings (from `[dpi]`, see contracts/config.md)

`binary`, `max_flows`, `max_packets_per_conn`, `idle_timeout_ms`, `memory_limit_mb`, `queue_capacity`, `queue_max_bytes`.

### Supervisor state

| Field | Type |
|---|---|
| `state` | `Disabled \| NotFound \| Starting \| Running \| Restarting \| Failed \| Incompatible` |
| `pid`, `started_ms`, `version` (`proto`, `ndpi`) | |
| `restarts` | ring of failure timestamps; 5 within 10 min → `Failed` |
| `last_stats` | latest `Stats` + receive time (heartbeat: 3 × 2 s missed → restart) |
| `catalog` | app names + categories from `Hello` (for the rule picker) |

### PacketTap (in `tmon-capture` processor)

`try_send(DpiPacket { key, dir, ts_ms, l3: Vec<u8> })` to a bounded `mpsc` with a byte budget.

- **Skips**: DNS (port 53), keys in `final_set` (LRU 100k), and connections with more than `max_packets_per_conn` packets forwarded (per-key counter LRU).
- **Counters**: `dpi_sent`, `dpi_dropped_queue`, `dpi_dropped_down` (sidecar not running), `dpi_skipped_final`.

### AppTable

A sharded LRU `ConnKey → (Arc<AppInfo>, last_used_ms)`.

- TTL = `idle_timeout_ms`; capacity `max_flows × 2`.
- It is the `ConnectionClassifier` implementation.
- Inserts follow `better_than`.

### PendingApps

A buffer of `(ConnKey, Arc<AppInfo>, ts_ms)`, drained every 5 s into `backfill_apps`. At most 10k per drain. Overflow is dropped and counted.

## 6. Storage (`tmon-store`)

| Change | Detail |
|---|---|
| Migration 7 | `ALTER TABLE flows_hot ADD COLUMN IF NOT EXISTS app VARCHAR; … app_confident BOOLEAN;` (`batch_stage` too) |
| `FLOW_COLUMNS` | `+ app, app_confident` at the end; `ADDED_FLOW_COLUMNS` extended |
| Old Parquet | read through `parquet_rows()` as NULL |
| `remote_host_source` | code 6 = `dpi` |
| `backfill_apps(store, now, rules, &[(ConnKey, AppInfo)])` | updates raw rows in `[now − 15 min, now]` whose 5-tuple matches and `app IS NULL`, then re-categorizes (same mechanics as `backfill_hostnames`) |
| Queries | `/flows?app=` filter (exact, case-insensitive); `by=app` breakdown and host top apps from raw only (`unsupported_filter_range` beyond raw retention) |

## 7. Validation summary

| Input | Rule | On failure |
|---|---|---|
| IPC message | contracts/dpi-ipc.md limits (type, lengths, counts, UTF-8/ASCII) | message dropped, `ipc_invalid` counted; 100 invalid in 10 s → restart sidecar |
| App/category names | printable ASCII 1–48 | entry ignored, `app_invalid` counted |
| Config `[dpi]` | contracts/config.md ranges; requires `capture.enabled` | startup error `dpi.<key>: …` |
| Rule `app` / `app_category` | 1–64 entries, 1–48 chars | `validation_failed` with a field error |
