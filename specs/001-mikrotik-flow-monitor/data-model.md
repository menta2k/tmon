# Data Model: MikroTik Flow Monitor (tmon)

**Feature**: [spec.md](./spec.md) | **Storage decision**: [research.md §R1](./research.md#r1-storage--does-victoriametrics-fit)

There are three storage tiers, all inside one data directory (`data_dir`, default `/var/lib/tmon`):

| Tier | Location | Contents | Retention |
|---|---|---|---|
| Raw | `raw/dt=YYYY-MM-DD/HH.parquet` + DuckDB `flows_hot` | Individual flow records | `retention.raw_days` (7) |
| Rollups | DuckDB `tmon.duckdb` tables `rollup_*_1m`, `rollup_*_1h` | Pre-aggregated totals | `retention.rollup_1m_days` (30), `retention.rollup_1h_days` (365) |
| Config/state | DuckDB tables `category`, `rule`, `local_host`, `local_network`, `exporter`, `remote_hostname`, `setting` | User settings, enrichment caches | Permanent / TTL |

---

## 1. FlowRecord (raw tier)

Normalized from NetFlow v5/v9/IPFIX/sFlow. Immutable once written, except for whole-file rewrites by re-categorization.

| Field | Type | Notes / validation |
|---|---|---|
| `ts` | TIMESTAMP (UTC, ms) | Flow end (or sFlow sample time); receive time if skew > 5 min |
| `ts_start` | TIMESTAMP | Flow start; `ts_start <= ts` |
| `exporter_id` | USMALLINT | FK → Exporter |
| `in_if`, `out_if` | UINTEGER | SNMP ifIndex, 0 if unknown |
| `ip_ver` | UTINYINT | 4 or 6 |
| `local_ip` | VARCHAR / INET (16-byte) | Address inside a LocalNetwork; for transit flows = src |
| `remote_ip` | INET | The other end |
| `local_port`, `remote_port` | USMALLINT | 0 for non-TCP/UDP |
| `proto` | UTINYINT | IANA protocol number |
| `tcp_flags` | UTINYINT | OR of flags |
| `bytes`, `packets` | UBIGINT | **Already multiplied** by sampling rate |
| `sampled` | BOOLEAN | true if sampling rate > 1 |
| `direction` | ENUM(`down`,`up`,`internal`,`transit`) | Derived from LocalNetwork (FR-007) |
| `nat_ip`, `nat_port` | INET / USMALLINT, nullable | Post-NAT address if exported |
| `category_id` | USMALLINT | FK → Category; never null ("Other" fallback) |
| `rule_id` | UINTEGER, nullable | Matching rule, null = fallback |
| `country` | VARCHAR(2), nullable | ISO-3166 alpha-2; null for private |
| `asn` | UINTEGER, nullable | |
| `as_name` | VARCHAR, nullable | Dictionary-encoded in Parquet |
| `remote_host` | VARCHAR, nullable | From the router DNS cache, at ingest time |

**Parquet layout**: sorted by `ts` within each file, zstd level 3, row group of 128k rows, dictionary encoding for the low-cardinality columns.

**Direction rules (FR-007)**: `src∈L ∧ dst∉L → up`, `src∉L ∧ dst∈L → down`, `both∈L → internal`, `neither → transit`. L is the union of LocalNetworks. For `up`, local = src. For `down`, local = dst.

## 2. Rollups (rollup tier)

Several narrow rollups are used instead of one full cube, which would explode in cardinality. Every rollup has `bucket TIMESTAMP`, `exporter_id`, `direction`, plus its own dimensions, and the measures `bytes UBIGINT, packets UBIGINT, flows UBIGINT`. Each exists at `_1m` and `_1h` grain. The primary key is (bucket, dims…), and rows are upserted with `INSERT … ON CONFLICT DO UPDATE SET bytes = bytes + excluded.bytes`.

| Rollup | Extra dimensions | Serves |
|---|---|---|
| `rollup_category` | `category_id`, `proto` | Dashboard totals, category / protocol breakdown |
| `rollup_host` | `local_ip`, `category_id` | Top talkers, host drill-down |
| `rollup_geo` | `country`, `asn`, `category_id` | Country/ASN breakdowns |
| `rollup_port` | `proto`, `remote_port` (top 1000/bucket, rest → 0) | Top ports |
| `rollup_remote` (1h only) | `local_ip`, `remote_ip`, `remote_host` (top 200 per host/hour) | Long-range top remote endpoints |

**Query routing**: The API chooses the cheapest source that covers the requested filters and range. If every filter dimension exists in one rollup, it uses the 1-min rollup for ranges up to 2 d and the 1-h rollup otherwise. If not, it uses the raw tier (only within raw retention); if the range exceeds raw retention, the API returns `422 unsupported_filter_range` with the list of dimensions that are supported.

## 3. Exporter

| Field | Type | Notes |
|---|---|---|
| `id` | USMALLINT PK | |
| `address` | INET | Source IP; unique together with `domain_id` |
| `domain_id` | UINTEGER | Observation domain / source ID / sFlow agent sub-id |
| `protocol` | ENUM(`netflow5`,`netflow9`,`ipfix`,`sflow5`) | Last seen |
| `name` | VARCHAR, nullable | User label |
| `sampling_override` | UINTEGER, nullable | ≥1; overrides the advertised rate |
| `count_direction` | ENUM(`ingress`,`egress`,`both`) | Avoids double counting (edge case) |
| `first_seen`, `last_seen` | TIMESTAMP | |

Runtime counters (`received`, `decoded`, `dropped{reason}`, `template_missing`, `parse_errors`) are kept in memory, exposed via `/metrics` and the Status API, and not persisted.

**State**: `unknown` (not in allow-list → dropped and counted), then `active` (received data in the last 5 min), then `stalled` (> 5 min with no data, which triggers the warning banner), then back to `active` when data resumes.

## 4. Category

| Field | Type | Validation |
|---|---|---|
| `id` | USMALLINT PK | |
| `name` | VARCHAR | Unique, 1–40 chars |
| `color` | VARCHAR | `#RRGGBB` |
| `description` | VARCHAR | ≤ 200 chars |
| `builtin` | BOOLEAN | Built-in categories can be renamed or recoloured, not deleted |

"Other" (id 0) is reserved and cannot be deleted.

## 5. Rule

| Field | Type | Validation |
|---|---|---|
| `id` | UINTEGER PK | |
| `category_id` | FK → Category | Must exist |
| `priority` | INTEGER | Higher wins. User rules live in the band 1,000,000+ and built-in rules below it, which enforces "user overrides built-in" |
| `enabled` | BOOLEAN | |
| `origin` | ENUM(`builtin`,`user`) | |
| `name` | VARCHAR | 1–80 chars |
| `match` | JSON (`RuleMatch`) | At least one criterion set |
| `hits_24h` | derived | From raw tier `count(*) where rule_id=…` (cached 5 min) |

`RuleMatch`: all set fields must match (AND). Lists inside a field are OR.

```text
proto?:        [u8]                  # e.g. [6, 17]
remote_port?:  [PortSpec]            # 443 | "27015-27030"
local_port?:   [PortSpec]
remote_cidr?:  [CIDR]                # IPv4/IPv6, validated
local_host?:   [IP | CIDR | MAC]     # MAC resolved via local_host table
asn?:          [u32]
country?:      [ISO2]
interface?:    [u32]                 # ifIndex
hostname?:     [glob]                # "*.googlevideo.com", case-insensitive
direction?:    [Direction]
```

## 6. LocalHost

| Field | Type | Notes |
|---|---|---|
| `ip` | INET PK | |
| `mac` | VARCHAR, nullable | `aa:bb:cc:dd:ee:ff`, from DHCP or user |
| `friendly_name` | VARCHAR, nullable | User-set; wins over `dhcp_hostname` |
| `dhcp_hostname` | VARCHAR, nullable | From the router integration |
| `first_seen`, `last_seen` | TIMESTAMP | |

Display name is `friendly_name ?? dhcp_hostname ?? ip`.

## 7. LocalNetwork

`cidr` (PK, validated), `label`. The default seed is 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 100.64.0.0/10 and fc00::/7.

## 8. RemoteHostname (enrichment cache)

`ip` (PK), `hostname`, `source` (`dns_cache`), `last_seen`. Rows are purged after 7 days without being seen.

## 9. Settings

Stored in the `setting` key/value table and overridable by the config file/env (see [contracts/config.md](./contracts/config.md)). The file is the bootstrap source. UI-editable keys (retention, local networks, router integration, sampling overrides) are persisted in the DB, and **DB values win** once set.

## Relationships

```text
Exporter 1─* FlowRecord *─1 Category 1─* Rule
FlowRecord *─1 LocalHost (by local_ip)      FlowRecord *─0..1 RemoteHostname (by remote_ip)
LocalNetwork (set) ── determines ── FlowRecord.direction / local_ip
```
