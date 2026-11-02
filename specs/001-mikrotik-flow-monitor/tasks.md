---

description: "Task list for MikroTik Flow Monitor (tmon)"
---

# Tasks: MikroTik Flow Monitor (tmon)

**Input**: Design documents from `/specs/001-mikrotik-flow-monitor/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/http-api.md, contracts/config.md, quickstart.md

**Tests**: INCLUDED. The plan's Constitution Check adopts TDD with ≥ 80 % coverage. In every phase, write the test tasks first and confirm they FAIL before starting the implementation tasks.

**Organization**: Tasks are grouped by user story. Phases follow priority order: US1 (P1), US2 (P1), US3 (P2), US5 (P2), US4 (P3), US6 (P3).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: User story from spec.md (US1–US6)
- Paths are relative to the repository root: `backend/` (Cargo workspace), `frontend/` (Vite + Vue), `deploy/`, `scripts/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and tooling

- [X] T001 Create directory layout from plan.md: `backend/crates/{tmon-core,tmon-collector,tmon-enrich,tmon-store,tmon-api,tmon}/src`, `backend/assets`, `backend/tests/fixtures`, `frontend`, `deploy`, `scripts`. Add a root `.gitignore` covering `target/`, `node_modules/`, `frontend/dist/`, `data/`, `*.duckdb*`, `*.parquet`, `.env` and run `git init`
- [X] T002 Create the workspace manifest `backend/Cargo.toml`: resolver 2, edition 2024, `rust-version = "1.85"`. Add `[workspace.dependencies]`: tokio (full), axum 0.8, tower-http (compression-gzip, trace, timeout, set-header), serde/serde_json, thiserror, anyhow, tracing/tracing-subscriber, netflow_parser 1, sflow-parser 0.7, duckdb (features `bundled`, `parquet`, `appender-arrow`), maxminddb 0.32, prefix-trie, globset, arc-swap, lru, reqwest (rustls-tls, json, default-features=false), rust-embed 8, figment (toml, env), argon2, rand, utoipa + utoipa-axum, socket2, pcap-file, csv, time. Add dev-dependencies proptest, criterion, wiremock, tempfile, tower (util). Set `[profile.release] lto = "thin", codegen-units = 1`
- [X] T003 [P] Create crate manifests and empty `lib.rs`/`main.rs` for each crate in `backend/crates/*/Cargo.toml`. Dependency direction: core ← collector, enrich, store ← api ← tmon (binary)
- [X] T004 [P] Add `backend/rustfmt.toml`, `backend/clippy.toml` and `backend/.cargo/config.toml` (alias `cargo lint = clippy --all-targets -- -D warnings`)
- [X] T005 [P] Scaffold the frontend with Vite + Vue 3.5 + TypeScript (strict) + pnpm in `frontend/`. Dependencies: vue-router, pinia, naive-ui, echarts, vue-echarts. Dev dependencies: vitest, @vue/test-utils, jsdom, @playwright/test, openapi-typescript, eslint + eslint-plugin-vue, prettier. Set `frontend/vite.config.ts` to proxy `/api` and `/metrics` to `http://localhost:8080`
- [X] T006 [P] Add a `justfile` at the repo root with recipes `build-ui`, `build` (ui then `cargo build --release --features embed-ui`), `test`, `lint`, `coverage` (`cargo llvm-cov --workspace --fail-under-lines 80` + `pnpm test:unit --coverage`), `dev` and `replay FILE`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The end-to-end skeleton. UDP flows are decoded, classified by direction, stored (raw + rollups) and queryable through an authenticated API with an empty UI shell. Categorization uses a fallback ("Other") until US1.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

### Tests (write first, must FAIL)

- [X] T007 [P] Build the pcap fixture corpus in `backend/tests/fixtures/` and document every file's origin in `backend/tests/fixtures/README.md`. Files:
  - `nf5.pcap`, `nf9.pcap`, `routeros7-ipfix.pcap` (real RouterOS v7 capture or generated)
  - `nf9-data-before-template.pcap`, `malformed.pcap`, `sflow-crs-1in512.pcap`
  - `mixed-home.pcap` + `mixed-home.truth.csv` (expected bytes per category/direction/host)
  - `steam.pcap` (UDP 27015–27030)
  
  Put the generation recipe (softflowd/nfdump `nfreplay`/scapy) in `scripts/gen-fixtures.md`
- [X] T008 [P] Write direction classification tests in `backend/crates/tmon-core/tests/direction.rs`. Cases: `src∈L ∧ dst∉L → up`, `src∉L ∧ dst∈L → down`, both∈L → internal, neither → transit. Check that local_ip selection is src for up and dst for down. Cover IPv6 ULA and the default seed networks 10/8, 172.16/12, 192.168/16, 100.64/10, fc00::/7
- [X] T009 [P] Write config tests in `backend/crates/tmon/tests/config.rs`:
  - empty `password_hash` → error mentioning `tmon hash-password`
  - an invalid listen address is rejected
  - a non-writable `data_dir` is rejected
  - `TMON_RETENTION__RAW_DAYS=3` env overrides the file
  - a missing GeoIP DB is only a warning
- [X] T010 [P] Write NetFlow/IPFIX decode tests in `backend/crates/tmon-collector/tests/netflow_decode.rs`:
  - `nf5`, `nf9` and `routeros7-ipfix` fixtures yield the expected FlowRecords (5-tuple, bytes, packets, timestamps)
  - `nf9-data-before-template` increments `template_missing` and decodes later records
  - `malformed.pcap` increments `parse_errors` without panicking
  - an exporter timestamp skewed > 5 min falls back to receive time
- [X] T011 [P] Write sFlow decode tests in `backend/crates/tmon-collector/tests/sflow_decode.rs`: `sflow-crs-1in512.pcap` gives bytes/packets ×512 with `sampled=true`, and both IPv4 and IPv6 headers decode
- [X] T012 [P] Write listener tests in `backend/crates/tmon-collector/tests/listener.rs`:
  - datagrams from outside a non-empty `allowed_exporters` are dropped with `dropped{reason="not_allowed"}`
  - when the queue is full, `dropped{reason="backpressure"}` increments and the listener keeps running
- [X] T013 [P] Write storage writer tests in `backend/crates/tmon-store/tests/writer.rs` using tempfile:
  - an appended batch is visible through the `flows` view
  - hour rotation writes `raw/dt=YYYY-MM-DD/HH.parquet` and truncates `flows_hot`
  - the view unions hot + Parquet
  - rollup upsert sums bytes for repeated keys in `rollup_category_1m` and `_1h`
- [X] T014 [P] Write query-router tests in `backend/crates/tmon-store/tests/query_router.rs`:
  - range ≤ 2 d with rollup-covered filters → `rollup_1m`
  - longer → `rollup_1h`
  - a non-covered filter within raw retention → `raw`
  - a non-covered filter beyond raw retention → `UnsupportedFilterRange` listing the supported dimensions
- [X] T015 [P] Write API foundation tests in `backend/crates/tmon-api/tests/auth.rs`:
  - login with a valid password sets an `HttpOnly; SameSite=Strict` cookie `tmon_session`
  - a wrong password → 401 `unauthorized`
  - the 6th login attempt within a minute from one IP → 429 `rate_limited`
  - a protected route without a cookie → 401
  - `Authorization: Bearer <api_token>` is accepted
  - `GET /api/v1/health` needs no auth
  - an unknown query parameter → 400 `validation_failed`
  - the error envelope shape matches contracts/http-api.md

### Implementation

- [X] T016 Implement the immutable `FlowRecord` in `backend/crates/tmon-core/src/flow.rs` with the fields exactly as in data-model.md §1:
  - `ts`, `ts_start` (`ts_start <= ts`)
  - `exporter_id`, `in_if`, `out_if`, `ip_ver` (4 or 6)
  - `local_ip`, `remote_ip`, `local_port`, `remote_port`, `proto`, `tcp_flags`
  - `bytes`, `packets` ("Already multiplied by sampling rate"), `sampled`
  - `direction` ENUM(`down`,`up`,`internal`,`transit`)
  - nullable `nat_ip`, `nat_port`
  - `category_id` ("never null ('Other' fallback)"), nullable `rule_id`
  - nullable `country` (ISO-3166 alpha-2), `asn`, `as_name`, `remote_host`
  
  Add a `RawFlow` pre-classification struct (src/dst form) and `with_*` copy-update methods. Do not add setters
- [X] T017 [P] Implement the shared error types (`thiserror`) in `backend/crates/tmon-core/src/error.rs`
- [X] T018 Implement `LocalNetworks` (immutable set, prefix-trie lookup) and `classify(RawFlow) -> (Direction, local, remote)` in `backend/crates/tmon-core/src/direction.rs`, using the rules from data-model.md "Direction rules"
- [X] T019 Implement the `Category` model in `backend/crates/tmon-core/src/category.rs`:
  - validation: name "Unique, 1–40 chars", color `#RRGGBB`, description "≤ 200 chars"
  - reserved `OTHER_ID = 0`
  - a `Categorizer` trait (`fn categorize(&FlowRecord) -> (u16, Option<u32>)`) and a `FallbackCategorizer` that always returns Other
- [X] T020 Implement config loading in `backend/crates/tmon/src/config.rs`: figment with `tmon.toml` and `TMON_<SECTION>__<KEY>` env, using the exact schema and defaults in contracts/config.md, plus startup validation (password_hash, listen addrs, writable data_dir, GeoIP warning)
- [X] T021 [P] Implement per-exporter atomic counters in `backend/crates/tmon-collector/src/counters.rs`: `received`, `decoded`, `dropped{reason}`, `template_missing`, `parse_errors`, plus a 10 s EWMA of flows/s. Expose an immutable `CountersSnapshot`
- [X] T022 Implement the exporter registry in `backend/crates/tmon-collector/src/exporter_registry.rs` per data-model.md §3:
  - key `(address, domain_id)` unique
  - `protocol` ENUM(`netflow5`,`netflow9`,`ipfix`,`sflow5`)
  - `sampling_override` "≥1; overrides advertised"
  - `count_direction` ENUM(`ingress`,`egress`,`both`)
  - `first_seen`/`last_seen`
  - state `unknown → active → stalled → active` (stalled = > 5 min with no data)
  
  Persist rows via the store trait
- [X] T023 Implement the NetFlow v5/v9/IPFIX decoder in `backend/crates/tmon-collector/src/netflow.rs`:
  - wrap `netflow_parser` with a parser/template cache per `(exporter, observation domain)`
  - normalize to `RawFlow`
  - apply the sampling rate (advertised or override)
  - map IPFIX NAT IEs (`postNATSourceIPv4Address`, `postNAPTSourceTransportPort`) to `nat_ip`/`nat_port`
  - fall back to receive time when skew > 5 min
  - count errors, never panic
- [X] T024 [P] Implement the sFlow v5 decoder in `backend/crates/tmon-collector/src/sflow.rs`: use `sflow-parser`, decode the sampled Ethernet/IPv4/IPv6/TCP/UDP header into `RawFlow`, scale by the sampling rate and set `sampled=true`. Ignore counter samples for flows but record the agent in the exporter registry
- [X] T025 Implement the UDP listeners in `backend/crates/tmon-collector/src/listener.rs`:
  - one tokio task per bind address, `SO_RCVBUF` set via socket2 from `socket_recv_buffer_bytes`
  - `allowed_exporters` filter
  - detect the version from the header (5/9/10 → netflow.rs, sFlow port → sflow.rs)
  - push into a bounded `mpsc` of `queue_capacity` using `try_send` (drop and count when full)
- [X] T026 Define the `FlowStore` trait in `backend/crates/tmon-store/src/lib.rs` (append_batch, flush, query APIs returning immutable result structs). Create the DuckDB schema and seeds in `backend/crates/tmon-store/src/duck/schema.rs`:
  - tables `exporter`, `category`, `rule`, `local_host`, `local_network`, `remote_hostname`, `setting`, `flows_hot`
  - rollups `rollup_{category,host,geo,port}_{1m,1h}` and `rollup_remote_1h`, with the PKs from data-model.md §2
  - seeds: category Other (id 0) and the default local networks
  - `PRAGMA memory_limit`/`threads` from config
  - versioned migrations table
- [X] T027 Implement the single-writer storage actor in `backend/crates/tmon-store/src/duck/writer.rs`: receive enriched batches, append to `flows_hot` via the Appender every `flush_interval_secs`, call rollup upsert, and flush on shutdown. Hand out cloned read connections
- [X] T028 Implement hourly rotation in `backend/crates/tmon-store/src/duck/rotate.rs`:
  - `COPY (SELECT … ORDER BY ts) TO 'raw/dt=YYYY-MM-DD/HH.parquet' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 131072)`
  - then truncate the hot rows for that hour
  - (re)create the view `flows` = `flows_hot UNION ALL read_parquet('raw/**/*.parquet', hive_partitioning=1)`
- [X] T029 Implement rollup maintenance in `backend/crates/tmon-store/src/duck/rollup.rs`:
  - incremental `INSERT … ON CONFLICT DO UPDATE SET bytes = bytes + excluded.bytes` (likewise packets/flows) for `rollup_category`, `rollup_host`, `rollup_geo` and `rollup_port` (top 1000 remote ports per bucket, rest → port 0) at 1m and 1h grain
  - `rollup_remote_1h` (top 200 remotes per local host per hour) computed at rotation
- [X] T030 Implement the query router in `backend/crates/tmon-store/src/duck/query/router.rs`: choose `rollup_1m` (≤ 2 d), `rollup_1h` or `raw` from the filter dimensions per data-model.md "Query routing", and return `UnsupportedFilterRange{supported_dims}` when needed. Add shared SQL builder helpers with bound parameters only (no string interpolation of user values) in `backend/crates/tmon-store/src/duck/query/sql.rs`
- [X] T031 Implement the enrichment pipeline in `backend/crates/tmon-enrich/src/pipeline.rs`:
  - worker(s) consume `RawFlow` from the collector channel
  - direction classification via an `ArcSwap<LocalNetworks>`
  - apply the exporter `count_direction` (drop flows outside the selected direction)
  - pluggable `GeoLookup` trait (no-op for now), `Categorizer` via `ArcSwap`
  - batch to the store actor
- [X] T032 Implement the API envelope and errors in `backend/crates/tmon-api/src/envelope.rs`:
  - `Envelope{ok,data,error,meta}`
  - `ApiError` codes → statuses exactly as the contracts/http-api.md table, with no internals leaked on `internal`
  - common filter extractor with `deny_unknown_fields`: `from`/`to` (RFC3339 or `now-24h`), `from < to`, "maximum span 400 d", `step`, repeatable `host/category/proto/direction/country/asn/exporter`, `limit` 1–1000 default 20
- [X] T033 Implement auth in `backend/crates/tmon-api/src/auth.rs` and `backend/crates/tmon-api/src/routes/auth.rs`:
  - argon2 verify of `http.password_hash`
  - in-memory session store with `session_ttl_hours` and a random 256-bit id
  - cookie `tmon_session` HttpOnly, SameSite=Strict, Path=/
  - Bearer `api_token` support
  - login limited to 5/min/IP
  - `POST /auth/login`, `POST /auth/logout`, `GET /health` (`{status, checks:{storage, geoip, collectors}}`)
- [X] T034 Implement the SPA serving in `backend/crates/tmon-api/src/spa.rs`: `rust-embed` of `../../frontend/dist` behind cargo feature `embed-ui`, history-mode fallback to `index.html` and cache headers for hashed assets. Assemble the router in `backend/crates/tmon-api/src/lib.rs` with tower-http trace/compression/timeout and security headers (CSP `default-src 'self'`, `X-Content-Type-Options`, `Referrer-Policy`)
- [X] T035 Implement the binary wiring in `backend/crates/tmon/src/main.rs` and `backend/crates/tmon/src/cli/{mod,serve,hash_password}.rs`:
  - `tmon serve [--config]` starts the collectors → pipeline → store → API
  - graceful shutdown on SIGINT/SIGTERM flushes the writer
  - `tmon hash-password` reads stdin and prints an argon2id hash
- [X] T036 [P] Implement `tmon replay <file.pcap> [--to addr] [--speed 1x|max]` in `backend/crates/tmon/src/cli/replay.rs`: read UDP payloads with `pcap-file` and resend them, preserving inter-packet timing at 1x
- [X] T037 [P] Implement the typed API client in `frontend/src/api/client.ts`: unwrap the envelope, throw `ApiError` with code/message/fields, redirect to `/login` on 401. Add script `gen:api` in `frontend/package.json` that runs openapi-typescript against `http://localhost:8080/api/v1/openapi.json` into `frontend/src/api/schema.d.ts`
- [X] T038 [P] Build the frontend shell:
  - `frontend/src/App.vue` (Naive UI layout, side nav, dark/light), `frontend/src/router.ts` (auth guard), `frontend/src/views/LoginView.vue`
  - stores `frontend/src/stores/timeRange.ts` (presets 15m/1h/24h/7d/30d/custom) and `frontend/src/stores/filters.ts` (immutable updates)
  - components `frontend/src/components/TimeRangePicker.vue` and `frontend/src/components/FilterBar.vue`
- [X] T039 Expose the OpenAPI document at `GET /api/v1/openapi.json` in `backend/crates/tmon-api/src/openapi.rs`, assembled with utoipa from the route annotations — *deviation: hand-built `serde_json` document; `tests/openapi.rs` asserts every routed path is documented*

**Checkpoint**: `tmon replay nf9.pcap` → rows visible in the `flows` view and the rollups. The authenticated API and the empty UI shell work.

---

## Phase 3: User Story 1 - See live traffic broken down by category (Priority: P1) 🎯 MVP

**Goal**: A dashboard with throughput over time and breakdowns by category, protocol and direction, refreshed live, for selectable time ranges.

**Independent Test**: Replay `mixed-home.pcap`. The dashboard shows per-category/direction bytes matching `mixed-home.truth.csv` within 1 % and updates at least every 10 s (quickstart V2, V5).

### Tests for User Story 1 (write first, must FAIL)

- [X] T040 [P] [US1] Write rule engine tests in `backend/crates/tmon-core/tests/rules.rs`:
  - every `RuleMatch` criterion (proto, remote_port/local_port single and `"27015-27030"` range, remote_cidr v4/v6, local_host IP/CIDR/MAC, asn, country, interface, hostname glob case-insensitive, direction)
  - AND across fields, OR within a list
  - higher priority wins
  - user band ≥ 1,000,000 beats built-in
  - disabled rules are skipped
  - no match → Other with `rule_id = None`
- [X] T041 [P] [US1] Write built-in rules tests in `backend/crates/tmon-core/tests/builtin_rules.rs`: `backend/assets/builtin-rules.toml` parses, every rule validates, and categories include Web, Streaming, Gaming, Messaging/VoIP, Social, Cloud/Storage, Software Updates, VPN/Tunnels, DNS, Email, Network Services, Other (FR-009)
- [X] T042 [P] [US1] Write a property test in `backend/crates/tmon-core/tests/rules_prop.rs` (proptest): categorization is deterministic and the compiled-index result equals a naive linear scan over the ordered rules
- [X] T043 [P] [US1] Write the US1 integration test in `backend/tests/us1_dashboard.rs`. Start the app on ephemeral ports, replay `mixed-home.pcap`, then check:
  - `GET /api/v1/traffic/breakdown?by=category` matches the truth CSV within 1 %
  - `GET /traffic/timeseries?group_by=direction` returns `{step, series}`
  - `meta.source` is reported
  - a sampled fixture sets `meta.sampled=true`
- [X] T044 [P] [US1] Write the SSE test in `backend/crates/tmon-api/tests/live.rs`: `GET /api/v1/live` emits `event: tick` every 5 s with `{ts, bytes_per_sec:{down,up}, by_category:[{id,bps}]}`
- [X] T045 [P] [US1] Write frontend unit tests `frontend/tests/unit/DashboardView.spec.ts` and `frontend/tests/unit/TimeSeriesChart.spec.ts`: renders series from a mocked API, reacts to time-range changes, shows the "estimated (sampled)" badge when `meta.sampled`
- [X] T046 [P] [US1] Write the Playwright e2e `frontend/tests/e2e/dashboard.spec.ts`: log in, replay a fixture, and assert the category breakdown is visible within 30 s and the time range switches (quickstart V2)

### Implementation for User Story 1

- [X] T047 [P] [US1] Implement the `Rule` and `RuleMatch` models in `backend/crates/tmon-core/src/rules/model.rs` exactly per data-model.md §5:
  - `priority` "Higher wins. User rules live in the band 1,000,000+ and built-in rules below it"
  - `origin` ENUM(`builtin`,`user`), `name` 1–80 chars
  - match requires "At least one criterion set"
  - `PortSpec` single or `a-b` range with a ≤ b
  - CIDR, ISO2 and MAC validation
- [X] T048 [US1] Implement the immutable `RuleSet` compiler in `backend/crates/tmon-core/src/rules/compile.rs` (depends on the model task):
  - sort by priority, desc
  - build indexes: prefix-trie for CIDRs, `HashMap<(proto,port)>` + range vector for ports, `HashMap` for asn/country, `globset` for hostnames
- [X] T049 [US1] Implement the matcher in `backend/crates/tmon-core/src/rules/matcher.rs` and `backend/crates/tmon-core/src/rules/mod.rs`: `RuleSet` implements `Categorizer`, evaluates candidates from the indexes in priority order (first full match wins) and falls back to Other
- [X] T050 [P] [US1] Author `backend/assets/builtin-rules.toml`:
  - the 12 categories with colours
  - port rules (53 DNS, 25/465/587/993/995 Email, 1194/51820/500/4500/1701/1723 VPN, 3478–3481/5060 VoIP, 27015–27030/3074/3478 Gaming, 123/161/67/68 Network Services, 80/443 Web as a low-priority catch)
  - ASN rules (e.g. AS2906 Netflix, AS32934 Meta → Social, AS16509/AS14618/AS8075 Cloud)
  - hostname rules (`*.googlevideo.com`, `*.nflxvideo.net`, `*.ttvnw.net` → Streaming; `*.windowsupdate.com`, `*.dl.delivery.mp.microsoft.com`, `swcdn.apple.com` → Software Updates; `*.whatsapp.net` → Messaging)
  
  All built-in priorities stay < 1,000,000
- [X] T051 [US1] Implement built-in seeding and the live rule snapshot in `backend/crates/tmon-enrich/src/ruleset_handle.rs`: on startup, upsert built-in categories/rules (origin builtin) into the store, compile a `RuleSet` from enabled rules and publish it via `ArcSwap`. Add a `reload()` used by later stories. Replace `FallbackCategorizer` in `pipeline.rs`
- [X] T052 [US1] Implement the traffic queries in `backend/crates/tmon-store/src/duck/query/traffic.rs`:
  - `timeseries(filter, group_by ∈ {category,direction,proto,country,exporter}, step)` with about 200 points by default, rounded to the source grain
  - `breakdown(filter, by ∈ {category,proto,direction,country,asn})` returning `{key,label,bytes,packets,flows,share}`
  - `meta.sampled` = any sampled exporter in the range
- [X] T053 [US1] Implement the routes `GET /api/v1/traffic/timeseries` and `GET /api/v1/traffic/breakdown` in `backend/crates/tmon-api/src/routes/traffic.rs`, with utoipa annotations and `meta.source`
- [X] T054 [US1] Implement the live aggregator and SSE in `backend/crates/tmon-api/src/live.rs`: subscribe to a `tokio::sync::broadcast` of per-5 s totals published by the pipeline (`backend/crates/tmon-enrich/src/live_tap.rs`), stream `event: tick` and `event: warning`, keep-alive 15 s
- [X] T055 [P] [US1] Implement the SSE composable `frontend/src/api/live.ts` (`useLive()`: reconnect with backoff, immutable state updates) and store `frontend/src/stores/live.ts`
- [X] T056 [P] [US1] Implement the chart components `frontend/src/components/charts/TimeSeriesChart.vue` (stacked area, legend toggles, category colours) and `frontend/src/components/charts/BreakdownChart.vue` (donut + table with share)
- [X] T057 [US1] Implement `frontend/src/views/DashboardView.vue`:
  - live throughput tiles (down/up)
  - timeseries grouped by category, with a direction toggle
  - breakdowns by category/protocol/direction
  - the "estimated (sampled)" badge
  - default range 15 m with 10 s refresh
  - wired to the `timeRange` and `filters` stores

**Checkpoint**: US1 is fully functional. This is the MVP demo.

---

## Phase 4: User Story 2 - Find out who is using the bandwidth (Priority: P1)

**Goal**: Rank local hosts by volume, drill into a host, and see remote endpoints with country/ASN/AS name.

**Independent Test**: Replay `mixed-home.pcap`. Top Hosts ranking and up/down split match the truth CSV, and the host drill-down lists the top 20 remotes with country/ASN, with private remotes labelled "Local/Private" (quickstart V6).

### Tests for User Story 2 (write first, must FAIL)

- [X] T058 [P] [US2] Write GeoIP tests in `backend/crates/tmon-enrich/tests/geoip.rs` using the MaxMind test databases vendored at `backend/tests/fixtures/geo/{GeoIP2-Country-Test.mmdb,GeoLite2-ASN-Test.mmdb}`:
  - known IP → country/asn/as_name
  - private/reserved → `None`
  - missing DB → `None` + warning
  - file replaced on disk → hot-reloaded
  - LRU hit path
- [X] T059 [P] [US2] Write the US2 integration test in `backend/tests/us2_top.rs`:
  - `GET /top/hosts` ranking and `bytes_down`/`bytes_up` match the truth
  - `GET /hosts/{ip}` returns totals, categories, protocols and ≤ 20 `top_remotes` enriched
  - `/top/remotes`, `/top/asns`, `/top/countries` and `/top/ports` honor `limit` and the filters
  - `/traffic/breakdown?by=country|asn` work
- [X] T060 [P] [US2] Write the Playwright e2e `frontend/tests/e2e/top-hosts.spec.ts`: open Top Hosts, click the first host, assert the drill-down shows remotes with flags/ASN and "Local/Private" (quickstart V6)

### Implementation for User Story 2

- [X] T061 [P] [US2] Implement the GeoIP lookup in `backend/crates/tmon-enrich/src/geoip.rs`:
  - `maxminddb` memory-mapped Country + ASN readers behind `ArcSwap`
  - file-mtime watcher for hot reload
  - `lru` cache of 65,536 entries
  - skip private/reserved/ULA/link-local
  - implements the `GeoLookup` trait and is wired into `pipeline.rs`
- [X] T062 [P] [US2] Implement `tmon geoip-update` in `backend/crates/tmon/src/cli/geoip_update.rs`: download the current-month DB-IP Lite `dbip-country-lite-YYYY-MM.mmdb.gz` and `dbip-asn-lite-YYYY-MM.mmdb.gz` (fall back to the previous month on 404), gunzip, then do an atomic rename into the paths from `[geoip]`. Print the CC BY 4.0 attribution
- [X] T063 [US2] Implement local host tracking in `backend/crates/tmon-enrich/src/hosts.rs`: upsert `local_host(ip, first_seen, last_seen)` for each batch's local IPs, keep an in-memory display-name map (`friendly_name ?? dhcp_hostname ?? ip`) behind `ArcSwap`, and add a store repo in `backend/crates/tmon-store/src/duck/repo/hosts.rs`
- [X] T064 [US2] Implement the top-N queries in `backend/crates/tmon-store/src/duck/query/top.rs`:
  - `top_hosts` (from `rollup_host`; `bytes_down`, `bytes_up`, flows, `top_category`)
  - `top_remotes` (`rollup_remote_1h` for long ranges, `raw` for ≤ raw retention; joined with `remote_hostname`)
  - `top_asns`, `top_countries` (`rollup_geo`), `top_ports` (`rollup_port`)
- [X] T065 [US2] Implement the host drill-down query in `backend/crates/tmon-store/src/duck/query/host.rs`: totals, timeseries, category split, protocol split and the top 20 remotes for one local IP
- [X] T066 [US2] Implement the routes `GET /api/v1/top/{hosts,remotes,asns,countries,ports}` in `backend/crates/tmon-api/src/routes/top.rs` and `GET /api/v1/hosts/{ip}` in `backend/crates/tmon-api/src/routes/hosts.rs`, with utoipa annotations
- [X] T067 [P] [US2] Implement the shared display helpers in `frontend/src/utils/format.ts` (bytes/bps, country flag emoji, "Local/Private" label for RFC1918/ULA) with unit tests in `frontend/tests/unit/format.spec.ts`
- [X] T068 [P] [US2] Implement `frontend/src/views/TopHostsView.vue` (sortable table: name, IP, down, up, flows, top category; row click → detail)
- [X] T069 [P] [US2] Implement `frontend/src/views/HostDetailView.vue` (host timeseries by category, category/protocol donuts, top-20 remotes table with hostname/country/ASN/AS name)
- [X] T070 [P] [US2] Implement `frontend/src/views/RemotesView.vue` (tabs: remotes, ASNs, countries, ports) and `frontend/src/components/charts/WorldMapChart.vue` (ECharts world map by country bytes)

**Checkpoint**: US1 and US2 both work independently.

---

## Phase 5: User Story 3 - Customize categories and name devices (Priority: P2)

**Goal**: CRUD for categories/rules/hosts/local networks, a rule dry-run, re-applying rules to history, and settings import/export.

**Independent Test**: Add the rule `UDP 27015-27030 → Gaming`, replay `steam.pcap`, see it under Gaming with `hits_24h > 0`, re-apply to history, and see historical bytes move. Rename a host and see the name everywhere (quickstart V7, V8).

### Tests for User Story 3 (write first, must FAIL)

- [X] T071 [P] [US3] Write the categories/rules API tests in `backend/crates/tmon-api/tests/rules_api.rs`:
  - create/update/delete with validation errors in `error.fields` (e.g. `remote_cidr[0]: invalid CIDR`, name length, colour format)
  - deleting a built-in category or Other → 409 `conflict`
  - `POST /rules/reorder` assigns user-band priorities in the given order
  - `POST /rules/test` returns `{category_id, rule_id}`
  - `GET /rules` includes `hits_24h`
  - a newly saved rule applies to flows received afterwards
- [X] T072 [P] [US3] Write the re-categorization tests in `backend/crates/tmon-store/tests/recategorize.rs`: after a rule change, re-apply over a range rewrites the affected Parquet hours and `flows_hot` with the new `category_id`/`rule_id` and rebuilds the matching `rollup_*` buckets. Job progress goes from 0 to 1, and a failure leaves the original files intact (write to a temp file, then atomic rename)
- [X] T073 [P] [US3] Write the hosts/networks/settings API tests in `backend/crates/tmon-api/tests/settings_api.rs`:
  - `PATCH /hosts/{ip}` sets `friendly_name`/`mac` (MAC format `aa:bb:cc:dd:ee:ff`)
  - `PUT /networks` full-replaces with CIDR validation and changes direction for new flows
  - `GET /settings/export` contains no secrets
  - `POST /settings/import` with one invalid item → 400 and nothing applied
  - a valid import is applied atomically
- [X] T074 [P] [US3] Write the Playwright e2e `frontend/tests/e2e/rules.spec.ts`: create the Gaming port-range rule, replay `steam.pcap`, verify the dashboard shows Gaming, run re-apply, rename a host and verify the name in Top Hosts (quickstart V7, V8)

### Implementation for User Story 3

- [X] T075 [P] [US3] Implement the category repository in `backend/crates/tmon-store/src/duck/repo/categories.rs` (CRUD; reject deleting `builtin` or id 0; built-in rename/recolour allowed)
- [X] T076 [P] [US3] Implement the rule repository in `backend/crates/tmon-store/src/duck/repo/rules.rs`: CRUD, `reorder(ids)` assigning priorities `1_000_000 + (n - i) * 10`, and `hits_24h` from `flows` grouped by `rule_id`, cached 5 min
- [X] T077 [P] [US3] Implement the local network and settings repositories in `backend/crates/tmon-store/src/duck/repo/{networks,settings}.rs`. Settings key/value: DB values override the file once set (data-model.md §9); the router password is stored but never returned
- [X] T078 [US3] Implement the re-categorization in `backend/crates/tmon-store/src/duck/recategorize.rs`: for each hour in range, `COPY (SELECT … with recomputed category via the provided Categorizer) TO tmp.parquet`, atomic rename, then delete and rebuild that hour's 1m/1h rollup rows. Process `flows_hot` in place. Report progress via a callback
- [X] T079 [US3] Implement the background job runner in `backend/crates/tmon-api/src/jobs.rs`: an in-memory job table `{state: queued|running|done|failed, progress 0..1, error?}` that runs one job at a time
- [X] T080 [US3] Implement the routes in `backend/crates/tmon-api/src/routes/rules.rs`:
  - `/categories` GET/POST, `/categories/{id}` PATCH/DELETE
  - `/rules` GET/POST, `/rules/{id}` PATCH/DELETE
  - `/rules/reorder`, `/rules/test`, `/rules/reapply`, `/jobs/{id}`
  - every mutation calls `ruleset_handle.reload()`
- [X] T081 [US3] Implement the routes in `backend/crates/tmon-api/src/routes/settings.rs`:
  - `GET /hosts` (paging), `PATCH /hosts/{ip}` (reloads the host display map)
  - `GET/PUT /networks` (swaps the `LocalNetworks` ArcSwap)
  - `GET/PATCH /settings`
  - `GET /settings/export`, `POST /settings/import` (validate the whole bundle first, then apply in one DuckDB transaction)
- [X] T082 [P] [US3] Implement `frontend/src/views/RulesView.vue` (rule list with drag-to-reorder, enable toggle, hits_24h, category column) and `frontend/src/components/RuleEditor.vue`:
  - form for every `RuleMatch` field with inline validation
  - "Test" panel calling `/rules/test`
  - "Re-apply to history" button with job progress
- [X] T083 [P] [US3] Implement `frontend/src/components/CategoryEditor.vue` (name, colour picker, description; delete disabled for built-ins)
- [X] T084 [P] [US3] Implement `frontend/src/views/HostsView.vue` (searchable list, inline edit of friendly name/MAC, first/last seen)
- [X] T085 [P] [US3] Implement `frontend/src/views/SettingsView.vue`: sections Local networks, Retention and disk cap, Export/Import (download JSON / upload with validation errors shown per field)

**Checkpoint**: US1–US3 work independently.

---

## Phase 6: User Story 5 - Know that collection is healthy (Priority: P2)

**Goal**: A status page, a stall warning banner, Prometheus metrics, exporter settings (sampling override, count direction), and retention/disk-cap enforcement made visible.

**Independent Test**: Replay a fixture including `malformed.pcap` and a non-allowed source, then check the Status counters. Stop the replay and the stall banner appears (with `TMON_TEST_STALL_SECS=10`). `/metrics` scrapes cleanly (quickstart V3, V10, V12).

### Tests for User Story 5 (write first, must FAIL)

- [X] T086 [P] [US5] Write the status API tests in `backend/crates/tmon-api/tests/status_api.rs`: `GET /status` matches the contract shape (exporters with protocol, flows_per_sec, last_seen, sampling, counters; collectors listening; ingest dropped by reason; storage disk/cap/oldest_raw; enrichment; warnings). The stall warning appears after `TMON_TEST_STALL_SECS` and clears when data resumes
- [X] T087 [P] [US5] Write the metrics tests in `backend/crates/tmon-api/tests/metrics.rs`: `/metrics` exposes `tmon_flows_received_total{exporter,proto}`, `tmon_flows_dropped_total{reason}`, `tmon_decode_errors_total`, `tmon_template_missing_total`, `tmon_bytes_total{category,direction}` and `tmon_storage_bytes` in Prometheus text format. Require the token when `api_token` is set
- [X] T088 [P] [US5] Write the retention tests in `backend/crates/tmon-store/tests/retention.rs`:
  - `dt=` directories older than `raw_days` are deleted
  - rollup rows older than `rollup_1m_days`/`rollup_1h_days` are deleted
  - when the disk cap is exceeded, the oldest raw hours are removed first and a warning is raised
- [X] T089 [P] [US5] Write the exporter settings tests in `backend/crates/tmon-api/tests/exporters_api.rs`: `PATCH /exporters/{id}` with `sampling_override` (≥ 1) scales new flows, and `count_direction=ingress` drops egress-reported duplicates
- [X] T090 [P] [US5] Write the Playwright e2e `frontend/tests/e2e/status.spec.ts` covering the Status page counters and the stall banner (quickstart V10)

### Implementation for User Story 5

- [X] T091 [P] [US5] Implement the health monitor in `backend/crates/tmon-collector/src/health.rs`: every 10 s, evaluate exporter states (stall threshold 300 s, overridable by `TMON_TEST_STALL_SECS`), produce immutable `Warning{code,message}` items (e.g. "No flows received from 192.168.88.1 for 5 min") and publish them to the live broadcast as `event: warning`
- [X] T092 [P] [US5] Implement the retention janitor in `backend/crates/tmon-store/src/duck/retention.rs`: run hourly and at startup, delete expired Parquet partitions and rollup rows, enforce `disk_cap_gb` by deleting the oldest raw hours, and report `disk_bytes` and `oldest_raw`
- [X] T093 [US5] Implement `GET /api/v1/status` in `backend/crates/tmon-api/src/routes/status.rs`, aggregating counters, registry, health warnings, storage stats and enrichment state (GeoIP DB date)
- [X] T094 [P] [US5] Implement `/metrics` in `backend/crates/tmon-api/src/metrics.rs` (hand-rolled Prometheus text encoder over the counter snapshots; bounded label sets only)
- [X] T095 [US5] Implement `GET /api/v1/exporters` and `PATCH /exporters/{id}` (`name`, `sampling_override`, `count_direction`) in `backend/crates/tmon-api/src/routes/exporters.rs`. Changes update the registry snapshot used by the decoders and the pipeline
- [X] T096 [P] [US5] Implement `frontend/src/views/StatusView.vue` (exporter table with editable name/sampling/count direction, collectors, drop counters by reason, storage gauge, enrichment status) and `frontend/src/components/WarningBanner.vue` (global, fed by `event: warning` plus a `/status` poll every 30 s), mounted in `App.vue`

**Checkpoint**: Collection health is visible, and retention/disk cap are enforced.

---

## Phase 7: User Story 4 - Enrich using router knowledge (Priority: P3)

**Goal**: An optional read-only RouterOS REST integration that pulls DHCP lease hostnames/MACs and DNS cache names, which also feed hostname rules.

**Independent Test**: Run `scripts/mock-routeros.py` and enable `[router]`. Host and remote hostnames appear, and a `*.example-video.com` rule matches. Stopping the mock shows an integration warning while ingest continues (quickstart V9).

### Tests for User Story 4 (write first, must FAIL)

- [X] T097 [P] [US4] Write the RouterOS client tests in `backend/crates/tmon-enrich/tests/routeros.rs` using `wiremock`:
  - parse `/rest/ip/dhcp-server/lease` JSON (`address`, `mac-address`, `host-name`)
  - parse `/rest/ip/dns/cache` JSON (`name`, `data`, `type` A/AAAA only)
  - basic auth header sent
  - 401/timeout → error state recorded, no panic
- [X] T098 [P] [US4] Write the integration test in `backend/tests/us4_router.rs`:
  - with the mock enabled, `/hosts/{ip}` shows `dhcp_hostname`
  - `/top/remotes` shows `hostname`
  - a user rule `hostname: ["*.example-video.com"] → Streaming` categorizes new flows
  - `friendly_name` still wins over `dhcp_hostname`
  - when the mock stops, `/status.enrichment.router_integration.error` is set and flows keep arriving
- [X] T099 [P] [US4] Create the mock server `scripts/mock-routeros.py` (stdlib `http.server`, basic auth, serves lease and DNS-cache JSON from `scripts/mock-routeros-data.json`)

### Implementation for User Story 4

- [X] T100 [US4] Implement the RouterOS REST client and pollers in `backend/crates/tmon-enrich/src/routeros.rs`:
  - reqwest with rustls, `verify_tls` option, basic auth
  - lease poll every `lease_poll_secs`, DNS poll every `dns_poll_secs`
  - record `last_ok` and `error`
  - only spawned when `router.enabled = true` (off by default, FR-013)
- [X] T101 [US4] Implement the remote hostname repository in `backend/crates/tmon-store/src/duck/repo/remote_hostnames.rs` (upsert `ip, hostname, source='dns_cache', last_seen`; purge rows not seen for 7 days). Keep an in-memory `ArcSwap<HashMap<IpAddr, Arc<str>>>` in `backend/crates/tmon-enrich/src/remote_names.rs`, used by the pipeline to set `remote_host`
- [X] T102 [US4] Update local hosts from leases in `backend/crates/tmon-enrich/src/hosts.rs` (`dhcp_hostname`, `mac`) and reload the display-name map, keeping `friendly_name` precedence
- [X] T103 [US4] Extend `backend/crates/tmon-api/src/routes/settings.rs` and `routes/status.rs`: `[router]` settings PATCH (password write-only, restarts the pollers) and `enrichment.router_integration {enabled,last_ok,error?}` in status, raising a warning on error
- [X] T104 [P] [US4] Add a "Router integration" section to `frontend/src/views/SettingsView.vue` (URL, user, password write-only, verify TLS, intervals, "Test connection") and show `remote_host` in `HostDetailView.vue` and `RemotesView.vue`

**Checkpoint**: Router names enrich all views, and hostname rules work.

---

## Phase 8: User Story 6 - Explore and export raw flows (Priority: P3)

**Goal**: Filtered, paged raw flow search and CSV export.

**Independent Test**: Filter by host + port and check that the rows match the fixture. The CSV export opens with a header row and is truncated at 100,000 rows with a marker line (quickstart V11).

### Tests for User Story 6 (write first, must FAIL)

- [X] T105 [P] [US6] Write the flows API tests in `backend/crates/tmon-api/tests/flows_api.rs`:
  - `GET /flows` newest first, `limit` ≤ 500 (501 → 400), paging `meta.total`
  - filters `remote_ip`, `port` plus the common filters
  - a range beyond raw retention → 422 `unsupported_filter_range`
  - `GET /flows/export.csv` streams a header and rows, and with more than 100,000 rows the last line is `# truncated at 100000 rows`
- [X] T106 [P] [US6] Write the Playwright e2e `frontend/tests/e2e/flows.spec.ts` (filter, page, export download) (quickstart V11)

### Implementation for User Story 6

- [X] T107 [US6] Implement the raw search in `backend/crates/tmon-store/src/duck/query/flows.rs` over the `flows` view with partition pruning on `dt`, bound parameters, `ORDER BY ts DESC LIMIT/OFFSET` and a count. Add a streaming iterator for export, capped at 100,000 rows
- [X] T108 [US6] Implement `GET /api/v1/flows` and `GET /api/v1/flows/export.csv` in `backend/crates/tmon-api/src/routes/flows.rs` (streaming body with the `csv` writer, `Content-Disposition: attachment; filename="tmon-flows-<from>-<to>.csv"`, and the truncation marker line)
- [X] T109 [P] [US6] Implement `frontend/src/views/FlowsView.vue` (FilterBar + remote IP/port inputs, paged Naive UI data table with category colour chips, Export CSV button using the same query)

**Checkpoint**: All user stories are independently functional.

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Hardening, performance validation, packaging, docs

- [X] T110 [P] Add cargo-fuzz targets `backend/fuzz/fuzz_targets/netflow.rs` and `backend/fuzz/fuzz_targets/sflow.rs` (arbitrary datagrams → decoder must not panic), and run each for at least 10 min
- [X] T111 [P] Add a criterion benchmark `backend/crates/tmon-enrich/benches/pipeline.rs` (classify + geo + categorize ≥ 100k flows/s/core) and `backend/crates/tmon-core/benches/rules.rs` (< 1 µs/flow with built-in rules)
- [X] T112 [P] Create `scripts/bench-ingest.sh`: run `tmon` in a `systemd-run --scope -p MemoryMax=2G -p CPUQuota=200%` cgroup, replay a large fixture at `--speed max` with ≥ 10k flows/s for 10 min, and assert `tmon_flows_dropped_total{reason="backpressure"} == 0` and RSS < 1 GB (quickstart V13, SC-002)
- [X] T113 [P] Create `scripts/seed-rollups.rs` (cargo `[[bin]]` in `backend/crates/tmon/src/bin/seed_rollups.rs`) to generate 30 days of realistic rollups + 24 h of raw data, and `scripts/bench-queries.sh` that asserts dashboard endpoints at 30 d return in < 2 s and a 24 h raw first page in < 3 s (quickstart V14, SC-004)
- [X] T114 [P] Create the packaging:
  - `deploy/Dockerfile` (multi-stage: node build UI → rust build with `embed-ui` → distroless/cc runtime, non-root, `EXPOSE 8080 2055/udp 4739/udp 6343/udp`, volume `/var/lib/tmon`)
  - `deploy/docker-compose.yml`
  - `deploy/tmon.service` (systemd, `DynamicUser`, `StateDirectory=tmon`, hardening options)
  - `deploy/tmon.example.toml`
- [X] T115 [P] Write `docs/routeros.md`: RouterOS v7 traffic-flow/IPFIX/sFlow setup and the read-only REST user. **Verify** the property names (`/ip traffic-flow ipfix` NAT fields, `active-flow-timeout`, switch sFlow path) against current MikroTik documentation and update `contracts/config.md` to match
- [X] T116 [P] Write `README.md`: overview, install (binary/Docker), quickstart, configuration reference, storage design and the VictoriaMetrics note (scraping `/metrics`), DB-IP attribution
- [X] T117 Run the security review: the security-reviewer agent over `backend/crates/tmon-api` and `tmon-collector`. Check auth, rate limit, CSP, no secrets in logs/exports, SQL parameterization and UDP input bounds. Fix CRITICAL/HIGH findings
- [X] T118 Run the coverage gate `just coverage` (backend and frontend ≥ 80 % lines) and add tests where it is short
- [X] T119 Run the quickstart.md validation scenarios V1–V15 and record the results in `specs/001-mikrotik-flow-monitor/validation.md`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies; start immediately
- **Foundational (Phase 2)**: Depends on Setup and BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Foundational
- **US2 (Phase 4)**: Depends on Foundational. It uses the categories from US1 for `top_category`, but works with the fallback "Other" if US1 is not done
- **US3 (Phase 5)**: Depends on Foundational and on the US1 rule engine (`rules/*`, `ruleset_handle.rs`)
- **US5 (Phase 6)**: Depends on Foundational only
- **US4 (Phase 7)**: Depends on Foundational. Hostname rules need the US1 rule engine, and hostnames display in the US2 views
- **US6 (Phase 8)**: Depends on Foundational only
- **Polish (Phase 9)**: Depends on all desired stories

### Story graph

```text
Setup → Foundational ─┬─> US1 ─┬─> US3
                      │        └─> US4 (hostname rules) ◄── US2 (views)
                      ├─> US2
                      ├─> US5
                      └─> US6
                                         all ─> Polish
```

### Within Each User Story

- Tests are written first and must FAIL
- Models → compile/indexes → services/queries → routes → UI
- Each story ends at its checkpoint, validated independently

### Parallel Opportunities

- Setup: the crate manifests, lint config, frontend scaffold and justfile tasks can run in parallel
- Foundational: all 9 test tasks are [P]. The error types, counters, sFlow decoder, replay CLI, API client and frontend shell tasks are [P] once `flow.rs` exists
- After Foundational: US1, US2, US5 and US6 can proceed in parallel. US3 and US4 start once US1's rule engine lands
- Frontend view tasks within a story are [P] because they are separate files

---

## Parallel Example: User Story 1

```bash
# Tests first (all [P]):
Task: "Rule engine tests in backend/crates/tmon-core/tests/rules.rs"
Task: "Built-in rules tests in backend/crates/tmon-core/tests/builtin_rules.rs"
Task: "Property test in backend/crates/tmon-core/tests/rules_prop.rs"
Task: "US1 integration test in backend/tests/us1_dashboard.rs"
Task: "SSE test in backend/crates/tmon-api/tests/live.rs"
Task: "Frontend unit tests DashboardView.spec.ts / TimeSeriesChart.spec.ts"

# Then in parallel:
Task: "Rule/RuleMatch models in backend/crates/tmon-core/src/rules/model.rs"
Task: "Author backend/assets/builtin-rules.toml"
Task: "SSE composable frontend/src/api/live.ts"
Task: "Charts TimeSeriesChart.vue / BreakdownChart.vue"
```

## Parallel Example: User Story 2

```bash
Task: "GeoIP lookup in backend/crates/tmon-enrich/src/geoip.rs"
Task: "geoip-update CLI in backend/crates/tmon/src/cli/geoip_update.rs"
Task: "TopHostsView.vue" ; Task: "HostDetailView.vue" ; Task: "RemotesView.vue + WorldMapChart.vue"
```

---

## Implementation Strategy

### MVP First (User Story 1)

1. Phase 1 Setup, then Phase 2 Foundational. At the checkpoint, flows are stored and queryable
2. Phase 3 US1, which gives the categorized live dashboard
3. **STOP and VALIDATE** with quickstart V1, V2, V5 against a real MikroTik router

### Incremental Delivery

1. Run in this order: US1 → US2 (who uses bandwidth) → US5 (health) → US3 (customization) → US4 (router names) → US6 (raw search)
2. Each step ends at its checkpoint and adds value without breaking the earlier stories

### Notes

- [P] = different files, no dependencies on incomplete tasks
- Keep files ≤ 400 lines typical (800 max) and split modules as they grow
- Use immutable patterns throughout: snapshots via `ArcSwap`, `with_*` copy updates, no shared mutable state outside the storage actor
- Commit after each task or logical group using conventional commits
