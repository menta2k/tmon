# Phase 0 Research: MikroTik Flow Monitor (tmon)

**Feature**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md) | **Date**: 2026-10-31

---

## R1. Storage — does VictoriaMetrics fit?

### Verdict: **No, not as the primary store.** Use embedded **DuckDB + Parquet** instead (VictoriaMetrics stays an optional scrape target).

### Why VictoriaMetrics does not fit

VictoriaMetrics is an excellent, very light time-series database, but it stores **numeric samples per label set (series)**, not records. Flow data does not match that model:

| Requirement | Effect on VictoriaMetrics | Fit |
|---|---|---|
| Raw flow search + CSV export (FR-018) | VM cannot store or return individual records; every flow would have to become a series | ❌ |
| Top-N remote endpoints / ports (FR-015) | Remote IP, remote port and ephemeral local port as labels create **unbounded cardinality** (hundreds of thousands of new series per day at home scale; millions at 10k flows/s). High churn makes RAM and index size grow, and VM docs advise against this | ❌ |
| Re-categorize history after rule changes (FR-023) | Series labels are immutable; you cannot rewrite `category` on past data | ❌ |
| Arbitrary combined filters (FR-017) | Only works for dimensions that were labels at write time | ⚠️ |
| Category / direction / host throughput over time (FR-014) | Low-cardinality rollups (category × direction × local host) are a perfect fit | ✅ |
| Retention / downsampling (FR-021) | Single global retention in the open-source edition; downsampling is an Enterprise feature | ⚠️ |

So VM could hold only the dashboard rollups, and you would still need a second store for raw flows. That means two services plus logic duplicated across them, which defeats the "lightweight" goal.

**VictoriaLogs** (same vendor) handles high-cardinality fields and would work for raw flows. Still, it would be a second service with its own query language (LogsQL over HTTP), it is immutable (no FR-023), and it still needs VM or self-made rollups for long-range charts.

### Decision: embedded DuckDB (via the `duckdb` crate, bundled) + hourly Parquet files

- **Raw flows**: buffered in memory, appended every ~5 s to a DuckDB table `flows_hot` for the current hour. At each hour boundary they are written with `COPY … TO` as a zstd-compressed Parquet file `data/raw/dt=YYYY-MM-DD/HH.parquet`, and the hot table is truncated. Queries read from a view: `flows_hot UNION ALL read_parquet('data/raw/**/*.parquet', hive_partitioning=1)`, with partition pruning on `dt`.
- **Retention (FR-020/022)**: delete whole Parquet files. This is O(1), predictable and needs no vacuum.
- **Re-categorization (FR-023)**: rewrite the affected Parquet hours with a new `category` column, then rebuild their rollups.
- **Rollups (FR-021)**: native DuckDB tables at 1-min and 1-h grain, updated incrementally at each flush. Old buckets are removed with a `DELETE` on bucket range (small tables, so this is cheap).
- **One process**: storage is in-process, so the "single binary" requirement (FR-025) holds and there is no network hop or extra service to operate.

**Sizing check (SC-007)**: 500 flows/s × 7 days ≈ 302 M rows. Parquet+zstd on flow data is typically 15–30 B/row, so about 5–9 GB. Rollups (see data-model) come to under 2 GB for 30 d of 1-min data plus 1 y of 1-h data. The total stays under the 20 GB target.

**Performance check (SC-002/SC-004)**: DuckDB's Appender ingests more than 1 M rows/s on one core, so 10k flows/s is about 1 % of capacity. Dashboard queries read the rollups (millions of rows, < 200 ms). Raw searches over 24 h scan about 43 M rows (at 500 f/s) with column pruning, which takes about 1–2 s on 2 cores.

**Risks and mitigations**:
- DuckDB allows one writer per database file. All writes go through a single storage task (actor), and reads use cloned connections in the same process.
- The bundled build makes the binary larger (~40 MB) and slows compilation. Use `sccache` and a release profile with `lto = "thin"`.
- Memory: set `PRAGMA memory_limit='512MB'` and `threads=2` (configurable) to stay inside the 1 GB budget.

### Alternatives considered

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **VictoriaMetrics only** | Tiny, fast, PromQL, Grafana-ready | No records, cardinality blow-up, immutable, see above | Rejected as primary |
| **VictoriaMetrics + VictoriaLogs** | Both light, built-in retention | 3 processes, two query languages, no re-categorization, logic split | Rejected |
| **ClickHouse** | The industry standard for flows (Akvorado, GoFlow2 stacks), TTL, materialized views, scales to ISP size | Separate heavy server (~1 GB+ RAM idle under load, more ops), exceeds the "lightweight / ≤1 GB" budget | **Upgrade path**: keep the storage behind a `FlowStore` trait so a ClickHouse backend can be added later |
| **SQLite** | Trivial, embedded | Row store; aggregating hundreds of millions of rows is far too slow | Rejected |
| **TimescaleDB/Postgres** | SQL, mature | Separate server, heavier, row-oriented without tuning | Rejected |

### Keeping VictoriaMetrics in the picture (optional)

tmon exposes `/metrics` in Prometheus format: its own health counters plus **bounded** gauges (bytes per category/direction/exporter). If you already run VictoriaMetrics/Grafana, point `vmagent` or a scrape config at it. This is the one job VM is good at here.

---

## R2. Flow protocol parsing (Rust)

- **Decision**: `netflow_parser` (v1.x; supports NetFlow v5/v7/v9 and IPFIX, with per-source template cache) for NetFlow/IPFIX. `sflow-parser` (v0.7) for sFlow v5. Both are wrapped behind an internal `FlowDecoder` trait that outputs a normalized `FlowRecord`.
- **Rationale**: Both are maintained crates on crates.io. Writing template-based IPFIX decoding by hand is error-prone. Keeping them behind a trait lets either crate be replaced, e.g. with `netgauze-flow-pkt`.
- **Alternatives**: `netgauze-flow-pkt` (heavier, more complete IPFIX IE registry; fallback if `netflow_parser` misses MikroTik fields). A hand-written parser was rejected.
- **MikroTik specifics** (RouterOS v7 `/ip/traffic-flow`):
  - It exports v1, v5, v9 and IPFIX. Use `version=ipfix` (or 9) so the export includes IPv6 and the NAT fields (`postNATSourceIPv4Address`, `postNAPTSourceTransportPort`, enabled via `/ip/traffic-flow/ipfix`).
  - Defaults are `active-flow-timeout=30m` and `inactive-flow-timeout=15s`. The quickstart recommends `active-flow-timeout=1m` so long flows are reported per minute (edge case: long-lived flows).
  - It does not export the MAC address by default. Hosts are keyed by IP, and the MAC comes from DHCP leases (R5).
  - sFlow is available only on switch-chip devices (CRS3xx etc., `/interface/ethernet/switch` sFlow in v7.x). It provides sampled packet headers, which are decoded to the same 5-tuple and scaled by the sampling rate (FR-004).
- **Testing**: build a pcap fixture corpus of real RouterOS exports (v5, v9, IPFIX, sFlow), use `proptest`/`cargo-fuzz` on decoders, and replay fixtures in integration tests.

## R3. Geo / ASN enrichment

- **Decision**: `maxminddb` crate reading `.mmdb` files. The default source is **DB-IP Lite** Country + ASN (CC BY 4.0, monthly, no account needed). **MaxMind GeoLite2** is supported if the user provides a license key. Files are memory-mapped and hot-reloaded on change. An in-process LRU cache (≈64k entries) holds recent results.
- **Alternatives**: iptoasn.com TSV (ASN only), and online APIs (rejected: privacy, offline requirement).

## R4. Categorization engine

- **Decision**: An ordered rule list compiled into an immutable `RuleSet` (swapped atomically via `arc-swap` on change), evaluated in priority order with first match winning. Lookups are indexed:
  - CIDR → `prefix-trie` / `ip_network_table` longest-prefix map
  - ports → `HashMap<(proto, port)>` plus a range vector
  - ASN/country → `HashMap`
  - hostname → compiled `globset`

  Built-in rules ship as an embedded TOML file (ports, well-known ASNs and hostname suffixes such as `*.googlevideo.com` → Streaming). User rules always have a higher priority band (FR-009/FR-011 ordering).
- **Rationale**: Deterministic, testable and fast (< 1 µs per flow). Immutable snapshots fit the project's immutability rule.
- **Alternatives**: nDPI (needs packet payloads, which flows do not have; rejected) and ML classification (out of scope).

## R5. Router integration (DHCP leases, DNS cache)

- **Decision**: Use the RouterOS v7 **REST API** (`GET /rest/ip/dhcp-server/lease`, `GET /rest/ip/dns/cache`) over HTTPS with a read-only user, via `reqwest` with rustls. Poll every 60 s (leases) and 30 s (DNS cache). Results are merged into a `remote_hostname` table keyed by IP with last-seen time.
- **Rationale**: REST is built into v7, needs no binary API library and is easy to mock in tests. A read-only group (`policy=read,api,rest-api`) limits the risk.
- **Alternatives**: `routeros_rust` (binary API, for v6). This was rejected for v1 and documented as an add-on.

## R6. Backend web stack

- **Decision**:
  - HTTP: `tokio` + `axum` 0.8 + `tower-http` (compression, trace, timeout)
  - JSON: `serde`
  - SPA: built Vue output embedded via `rust-embed`
  - Config: `figment` (TOML + `TMON_*` env)
  - Logging: `tracing`
  - Auth: `argon2` password hash, HttpOnly SameSite=Strict session cookie, login rate limit
  - Live updates: Server-Sent Events (`/api/v1/live`)
- **Rationale**: These are the mainstream, well-maintained choices. SSE is simpler than WebSocket for one-way, 5–10 s updates.

## R7. Frontend stack

- **Decision**:
  - Core: Vue 3 + TypeScript + Vite, Pinia, Vue Router
  - Charts: **Apache ECharts** via `vue-echarts` (time series, stacked area, sankey, treemap, world map)
  - UI: **Naive UI** components
  - Tests: Vitest + Vue Test Utils for units, **Playwright** for e2e
  - API types: generated with `openapi-typescript` from the OpenAPI document the backend emits via `utoipa` (`/api/v1/openapi.json`), kept in sync with [contracts/http-api.md](./contracts/http-api.md)
- **Alternatives**: Chart.js (weaker for large time series and maps), PrimeVue/Vuetify (heavier bundles).

## R8. Ingest pipeline & back-pressure

- **Decision**: One `tokio::net::UdpSocket` task per listener with an enlarged `SO_RCVBUF` (8 MB, configurable). Decoded records go to a bounded `mpsc` (capacity 65k) → enrichment + categorization worker pool → batcher → storage actor. When the channel is full, records are dropped and the `dropped_total{reason="backpressure"}` counter is incremented (FR-006).
- **Time attribution**: the flow's end timestamp from the exporter, falling back to receive time if the skew is over 5 min (edge case).
