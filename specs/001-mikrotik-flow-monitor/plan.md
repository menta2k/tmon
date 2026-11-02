# Implementation Plan: MikroTik Flow Monitor (tmon)

**Branch**: `001-mikrotik-flow-monitor` | **Date**: 2026-10-31 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-mikrotik-flow-monitor/spec.md`

## Summary

tmon is a single-user, self-hosted flow monitor for MikroTik routers. One Rust binary does the following:

- receives NetFlow v5/v9, IPFIX and sFlow v5 over UDP
- normalizes, enriches (direction, GeoIP/ASN, router DHCP/DNS names) and categorizes each flow with an ordered rule engine
- stores the flows in an **embedded DuckDB + hourly Parquet** store with 1-min/1-h rollups
- serves a REST/SSE API plus an embedded Vue 3 dashboard

**Storage verdict**: VictoriaMetrics was evaluated and **rejected as the primary store**. It cannot hold individual flow records (raw search/CSV), high-cardinality dimensions (remote IP/ports) explode its series count, and its labels are immutable, so rule changes cannot be re-applied to history. Embedded DuckDB/Parquet is lighter (no extra service) and supports all of these. VictoriaMetrics can still scrape tmon's bounded `/metrics`. Full analysis: [research.md §R1](./research.md#r1-storage--does-victoriametrics-fit).

## Technical Context

**Language/Version**: Rust stable ≥ 1.88 (edition 2024) for the backend. TypeScript 5.x + Vue 3.5 for the frontend.

**Primary Dependencies**:
- Backend:
  - Runtime and HTTP: tokio, axum 0.8, tower-http
  - Flow decoding: netflow_parser (NetFlow/IPFIX), sflow-parser (sFlow v5)
  - Storage: duckdb (bundled)
  - Enrichment: maxminddb
  - Rule matching: prefix-trie, globset, arc-swap
  - Router integration: reqwest (rustls)
  - Supporting: rust-embed, figment, serde, tracing, argon2, utoipa
- Frontend: Vite, Vue Router, Pinia, Naive UI, ECharts (vue-echarts), openapi-typescript.

**Storage**: Embedded DuckDB (`tmon.duckdb`: rollups and config) + zstd Parquet files partitioned by date/hour (raw flows). Details: [data-model.md](./data-model.md).

**Testing**:
- Backend: cargo test, proptest and cargo-fuzz (decoders), pcap replay integration tests, criterion benches, cargo-llvm-cov (≥ 80 %).
- Frontend: Vitest + Vue Test Utils, Playwright e2e.

**Target Platform**: Linux x86_64/aarch64 (bare metal, VM, Docker); running as a container on the MikroTik itself is out of scope for v1. Desktop browsers.

**Project Type**: Web application (Rust backend service + Vue SPA embedded into the binary).

**Performance Goals**: 10k flows/s sustained ingest with zero loss, dashboard < 2 s for ≤ 30 d ranges, data visible within 30 s.

**Constraints**: ≤ 1 GB RSS on 2 cores/2 GB RAM, ≤ 20 GB disk at 500 flows/s with default retention, fully offline (no third-party calls apart from the optional `geoip-update`), single process.

**Scale/Scope**: 1 user, 1–5 exporters, ≤ 1,000 local hosts, about 300 M raw rows in retention, about 10 UI views.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

`.specify/memory/constitution.md` is still the **unfilled template**, so there are no ratified project principles. Until `/speckit-constitution` is run, the user's global engineering rules are applied as provisional gates:

| Gate | Pre-research | Post-design | Evidence |
|---|---|---|---|
| TDD, ≥ 80 % coverage, unit + integration + e2e | ✅ planned | ✅ | quickstart "Automated test commands"; pcap fixtures; Playwright |
| Immutability | ✅ | ✅ | RuleSet compiled into immutable snapshots swapped via arc-swap; FlowRecord immutable after decode; Parquet is append/replace-only |
| Small focused files (≤ 400 typical, 800 max) | ✅ | ✅ | Module layout below, organized by domain |
| Input validation at boundaries | ✅ | ✅ | Untrusted UDP decoded with bounds checks + fuzzing; API params validated, unknown params → 400; settings import fully validated before apply |
| Error handling, no silent swallowing | ✅ | ✅ | Every drop/decode error is counted and visible in Status/metrics; uniform error envelope |
| Security: no hardcoded secrets, auth, rate limiting | ✅ | ✅ | argon2 hash from env, router password write-only, login rate limit, SameSite=Strict cookie, no secrets in settings export |
| Research & reuse before writing | ✅ | ✅ | Existing crates for all protocol parsing, GeoIP and the DB (research R2–R7) |
| Simplicity | ✅ | ✅ | One process, embedded storage; no extra DB service |

**Result: PASS.** No violations, so Complexity Tracking is empty.

## Project Structure

### Documentation (this feature)

```text
specs/001-mikrotik-flow-monitor/
├── spec.md
├── plan.md              # this file
├── research.md          # Phase 0 — storage evaluation and tech choices
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1 — validation scenarios V1–V15
├── contracts/
│   ├── http-api.md      # REST/SSE/metrics contract
│   └── config.md        # UDP ports, tmon.toml, CLI, RouterOS setup
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks — not created yet)
```

### Source Code (repository root)

```text
backend/                          # Cargo workspace
├── Cargo.toml
├── crates/
│   ├── tmon-core/                # pure domain logic, no I/O
│   │   └── src/{flow.rs, direction.rs, category.rs, rules/{mod,model,compile,matcher}.rs, error.rs}
│   ├── tmon-collector/           # UDP listeners + decoders → FlowRecord
│   │   └── src/{listener.rs, netflow.rs, sflow.rs, exporter_registry.rs, counters.rs}
│   ├── tmon-enrich/              # GeoIP/ASN, local hosts, router REST integration
│   │   └── src/{geoip.rs, hosts.rs, routeros.rs, pipeline.rs}
│   ├── tmon-store/               # FlowStore trait + DuckDB/Parquet impl
│   │   └── src/{lib.rs (trait), duck/{writer,rollup,query,retention,recategorize,schema}.rs}
│   ├── tmon-api/                 # axum routes, auth, SSE, metrics, SPA embedding
│   │   └── src/{routes/{traffic,top,hosts,flows,rules,settings,status,auth}.rs, envelope.rs, auth.rs, live.rs, spa.rs}
│   └── tmon/                     # binary: config, CLI (serve, hash-password, geoip-update, replay), wiring
│       └── src/{main.rs, config.rs, cli/*.rs}
├── assets/builtin-rules.toml
└── tests/                        # integration: pcap replay → API assertions
    └── fixtures/*.pcap (+ ground-truth CSVs)

frontend/
├── src/
│   ├── api/                      # generated types + typed client, SSE composable
│   ├── stores/                   # pinia: timeRange, filters, live, settings
│   ├── components/               # charts (TimeSeries, Breakdown, Sankey, WorldMap), FilterBar, RuleEditor…
│   ├── views/                    # Dashboard, TopHosts, HostDetail, Remotes, Flows, Rules, Hosts, Status, Settings, Login
│   └── router.ts
└── tests/{unit,e2e}/

deploy/  {tmon.example.toml, Dockerfile, docker-compose.yml, tmon.service}
scripts/ {mock-routeros.py, seed-rollups.rs, gen-fixtures.md}
```

**Structure Decision**: The project is a web application split into `backend/` (Cargo workspace) and `frontend/` (Vite). The release build embeds `frontend/dist` into the `tmon` binary with `rust-embed`, so deployment is a single file. `tmon-core` has no I/O and holds all categorization/direction logic, which keeps it highly testable. `tmon-store` hides storage behind a `FlowStore` trait so that a ClickHouse backend can be added later without touching the API.

## Architecture & Data Flow

```text
MikroTik ──UDP 2055/4739 (NF v5/v9/IPFIX)──┐
         ──UDP 6343 (sFlow v5)─────────────┤
                                           ▼
   [collector] listener task/socket → decoder (template cache per exporter+domain)
        │ FlowRecord (raw)        counters → /metrics, /status
        ▼ bounded mpsc (65k, drop+count when full)
   [enrich] direction(LocalNetworks) → GeoIP/ASN (LRU) → remote_hostname(DNS cache) → RuleSet (arc-swap snapshot)
        ▼
   [store actor] batch 5 s → Appender → flows_hot ─┐ hourly COPY → raw/dt=…/HH.parquet
                 rollup upsert (1m, 1h)           │ janitor: retention + disk cap
                 live aggregator ──► SSE /live    │
        ▲                                         │
   [api] axum: query router (rollup_1m | rollup_1h | raw view) ◄──┘
        ▲
   Vue SPA (embedded)       RouterOS REST ◄── poller (leases 60 s, DNS 30 s), optional
```

## Delivery Phases (input for /speckit-tasks)

1. **Foundation**: workspace, config + validation, `tmon-core` types, direction logic, rules engine with built-in rules (TDD), pcap fixture corpus.
2. **Collection (US5 core)**: NetFlow/IPFIX/sFlow decoders, exporter registry, counters, `replay` CLI.
3. **Storage**: DuckDB schema, writer actor, Parquet rotation, rollups, query router, retention janitor.
4. **API + UI MVP (US1, US2)**: auth, status, timeseries/breakdown/top/host endpoints, SSE. Vue dashboard, top hosts, host detail, status, login. **This is the MVP milestone.**
5. **Customization (US3)**: categories/rules/hosts/networks CRUD, rule test, re-apply job, settings import/export.
6. **Raw flows (US6)**: search + CSV streaming.
7. **Router integration (US4)**: REST poller, mock server, hostname rules.
8. **Hardening**: fuzzing, throughput/RSS benchmark (V13), query benchmark (V14), Docker/systemd packaging, docs.

## Complexity Tracking

No constitution violations to justify.
