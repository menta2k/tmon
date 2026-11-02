# Implementation Plan: Application Classification with nDPI (Optional Sidecar)

**Branch**: `003-ndpi-app-classification` | **Date**: 2026-11-01 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/003-ndpi-app-classification/spec.md`

## Summary

Add optional application classification for traffic that announces no name (P2P, VPNs, games, calls, remote desktop).

- **Sidecar**: a new binary `tmon-dpi` (Rust, separate Cargo workspace `backend/dpi/`) wraps **nDPI 6.0** (dynamic link, LGPL-3.0). It runs as a supervised, sandboxed child process.
- **Channel**: tmon's feature 002 capture processor taps each decoded L3 packet into a bounded queue. The queue feeds the sidecar over a private `SOCK_SEQPACKET` socketpair with a documented binary protocol (`tmon-dpi-proto`).
- **Results**: the sidecar returns a classification for each LAN-side 5-tuple: app, nDPI category, confident/guess, host, and whether it is final.
- **Enrichment**: tmon stores the classifications in an `AppTable`, a `ConnectionClassifier` used by the enrichment pipeline after feature 002's naming.
- **Rules and storage**: rules gain `app` and `app_category` fields, with confidence "detected" or "inferred". Built-in mappings and a new *File Sharing* category ship with the release. `app`/`app_confident` are persisted per raw flow (migration 7), shown in Flows, the drill-down, a `by=app` breakdown and the rule editor, and back-filled for late classifications.
- **Supervision**: spawn, handshake and version check, heartbeat, back-off restart and a failed state. Status card, warnings and `/metrics` report it.
- **Fail-safe**: everything degrades to feature 002 behaviour when the sidecar is down, and tmon itself never links nDPI.

## Technical Context

**Language/Version**: Rust 1.9x stable (2024 edition) for tmon and the sidecar; C (nDPI 6.0, built from the pinned tag) only inside the sidecar; TypeScript 5 / Vue 3 for the UI.

**Primary Dependencies**:
- **New in tmon**: `command-fds` (pass fd 3 to the child), `nix` (socketpair `SOCK_SEQPACKET`, signals; or the `rustix` already in the tree, if present).
- **New in the sidecar**:
  - nDPI 6.0 through an internal `ndpi-sys` (bindgen, pkg-config);
  - `landlock` and `seccompiler` for the sandbox;
  - `libc` for `prctl`/`setrlimit`.
- **Shared**: a new pure-Rust crate `tmon-dpi-proto` with no dependencies beyond `thiserror`.
- Details are in [research.md](research.md) R1–R3 and R8.

**Storage**: DuckDB/Parquet. Migration 7 adds `app VARCHAR` and `app_confident BOOLEAN` to raw flows. Old Parquet files read as NULL via `parquet_rows()`. There is no new rollup; app views are raw-only (R11).

**Testing**:
- **`cargo test`**: codec unit and property tests; supervisor and back-pressure tests using a **fake sidecar** (`fake_dpi` example); rule and confidence tests; migration 7; API; end-to-end with the fake sidecar.
- **In `backend/dpi`**: integration tests with the real nDPI on generated app fixtures (SC-002), plus sandbox checks.
- **`cargo fuzz`**: two codec targets in the main workspace, and `sidecar_packets` (ASan) in `backend/dpi/fuzz`.
- **Vitest and Playwright**: app tag/filter, DPI status card, rule-editor app picker.

**Target Platform**: Linux x86_64/arm64, native and Docker (`tmon:*-dpi` image). Other OSes report DPI as unavailable.

**Project Type**: Web application (existing `backend/` workspace + `frontend/` SPA), plus one auxiliary binary in its own workspace.

**Performance Goals**:
- Sidecar ≥ 20k packets/s on one core.
- The tap keeps the capture bench at ≥ 50k datagrams/s.
- Flow ingest within 5 % of feature 002 (SC-003).
- Rule evaluation stays < 1 µs per flow (feature 001 bench).

**Constraints**:
- Sidecar memory < 256 MB at defaults: an allocator budget of 192 MB plus `RLIMIT_AS`.
- tmon adds ≤ 16 MB (SC-005).
- Zero flow loss on sidecar crash, hang or flood (FR-002, SC-004).
- No payload persisted or logged (FR-024).
- Off by default and requires capture (FR-021).
- The sidecar has no network and no file access after init (FR-023).

**Scale/Scope**: One router; ≤ 200 new connections/s at home; 50k tracked flows in the sidecar and 100k app entries in tmon (configurable).

The user-visible changes are:
- the `[dpi]` config;
- the `tmon-dpi` binary and `-dpi` image;
- the app column, filter and CSV columns;
- the app breakdown and host top apps;
- the app rule fields and picker;
- the DPI status card, warnings and metrics;
- docs (`docs/dpi.md`, router reply-direction rule, licensing).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

`.specify/memory/constitution.md` is still the template. The user's global rules are applied as provisional gates, as in features 001 and 002.

| Gate | Pre-research | Post-design | Evidence |
|---|---|---|---|
| TDD, ≥ 80 % coverage, unit + integration + e2e | ✅ planned | ✅ | The codec and supervisor are fully testable without C (fake sidecar). The real-nDPI suite is separate. The coverage gate covers the main workspace; the sidecar workspace has its own ≥ 80 % gate excluding `ndpi-sys` |
| Immutability | ✅ | ✅ | `AppInfo` is an immutable `Arc`. `AppTable` does insert/replace only. `FlowRecord::with_app` returns a new record |
| Small focused files | ✅ | ✅ | One module per concern (layout below); the sidecar `unsafe` code is confined to `ndpi.rs` |
| Input validation at boundaries | ✅ | ✅ | The IPC is untrusted in **both** directions: strict decode limits (contracts/dpi-ipc.md), name validation, rate limits, fuzzing (R12) |
| Error handling, no silent swallowing | ✅ | ✅ | Every drop, eviction, invalid message, restart and state change is counted and visible (Status, warnings, metrics) |
| Security | ✅ | ✅ | No secrets. Private socketpair. Sandbox: no-new-privs, setuid when root, Landlock, seccomp, `RLIMIT_AS`. LGPL isolation. Off by default |
| Research & reuse before writing | ✅ | ✅ | nDPI for classification; `landlock`/`seccompiler`/`command-fds` instead of hand-rolled code; feature 002 patterns reused (bounded queue, sharded LRU table, back-fill, `parquet_rows`) |
| Simplicity | ⚠️ | ✅ justified | A second process and a second workspace are added on purpose; see Complexity Tracking |

**Result: PASS**, with one justified complexity item.

## Project Structure

### Documentation (this feature)

```text
specs/003-ndpi-app-classification/
├── spec.md
├── plan.md                    # this file
├── research.md                # R1–R14
├── data-model.md
├── quickstart.md              # V1–V11
├── contracts/
│   ├── dpi-ipc.md             # tmon ↔ classifier protocol v1 (public)
│   ├── http-api.md            # /flows, by=app, /hosts, /applications/catalog, /status.dpi, warnings, metrics
│   ├── config.md              # [dpi]
│   └── router-capture.md      # optional reply-direction mangle rule
├── checklists/requirements.md
└── tasks.md                   # /speckit-tasks output
```

### Source Code (repository root)

```text
backend/
├── crates/
│   ├── tmon-core/src/
│   │   ├── app.rs                     # NEW: AppInfo, GENERIC_CARRIERS, trait ConnectionClassifier, name validation
│   │   ├── naming.rs                  # NameSource::Dpi (6)
│   │   ├── flow.rs                    # + app, app_confident, app_category(skip); with_app
│   │   └── rules/{model,matcher,confidence,compile}.rs   # app / app_category fields, index, confidence_for
│   ├── tmon-dpi-proto/                # NEW pure-Rust codec (both directions), limits, property tests
│   ├── tmon-dpi-client/               # NEW: tmon side
│   │   ├── src/
│   │   │   ├── settings.rs            # DpiSettings
│   │   │   ├── supervisor.rs          # spawn, handshake, heartbeat, back-off, states
│   │   │   ├── spawn.rs               # socketpair + fd 3 + clean env (only unsafe-free APIs)
│   │   │   ├── io.rs                  # writer task (queue → socket), reader task (socket → AppTable/PendingApps/stats)
│   │   │   ├── tap.rs                 # PacketTap given to tmon-capture (bounded, byte budget, final set)
│   │   │   ├── table.rs               # AppTable (ConnectionClassifier)
│   │   │   ├── pending.rs             # PendingApps for back-fill
│   │   │   ├── health.rs              # counters, DpiStatus snapshot, warnings
│   │   │   └── lib.rs                 # DpiHandle
│   │   ├── examples/fake_dpi.rs       # scripted fake classifier for tests
│   │   └── tests/                     # supervisor, backpressure, handshake, table
│   ├── tmon-capture/src/
│   │   ├── processor/mod.rs           # optional tap call after decode; LocalNetworks orientation for reply packets
│   │   └── fixtures/apps.rs           # NEW: BitTorrent, WireGuard, OpenVPN, STUN/RTP, Steam A2S, RDP generators
│   ├── tmon-enrich/src/pipeline.rs    # optional Arc<dyn ConnectionClassifier>; app step after naming
│   ├── tmon-store/src/duck/
│   │   ├── schema.rs                  # migration 7
│   │   ├── ingest.rs, rows.rs         # app columns
│   │   ├── backfill_apps.rs           # NEW
│   │   └── query/{flows,traffic,host}.rs   # app filter, by=app, top_apps
│   ├── tmon-api/src/
│   │   ├── routes/{dpi,applications}.rs     # /status.dpi section, /applications/catalog
│   │   ├── warnings.rs, metrics_dpi.rs
│   │   └── openapi.rs
│   └── tmon/src/{config_dpi.rs, dpi.rs, app.rs}   # [dpi] config, wiring, back-fill feed
├── dpi/                               # NEW separate workspace (needs nDPI + C toolchain)
│   ├── Cargo.toml
│   ├── ndpi-sys/                      # bindgen against pinned nDPI 6.0 headers (build.rs, pkg-config)
│   ├── tmon-dpi/src/
│   │   ├── main.rs                    # args, init, sandbox, run loop
│   │   ├── ndpi.rs                    # the ONLY unsafe module: module/flow RAII, process, giveup, names
│   │   ├── alloc.rs                   # counting allocator hooks + budget
│   │   ├── flows.rs                   # DpiFlow LRU, idle sweep, memory eviction
│   │   ├── classify.rs                # nDPI result → Classification (carrier filter, confidence map)
│   │   ├── sandbox.rs                 # no_new_privs, setuid, landlock, seccomp, rlimit
│   │   └── ipc.rs                     # socket loop using tmon-dpi-proto
│   ├── tmon-dpi/tests/                # real-nDPI app fixtures (SC-002), sandbox, memory budget
│   └── fuzz/fuzz_targets/sidecar_packets.rs
├── fuzz/fuzz_targets/{dpi_proto_to_sidecar,dpi_proto_to_tmon}.rs
├── assets/builtin-rules.toml          # File Sharing category + app rules (860k/850k bands)
└── tests/us8_dpi.rs                   # end-to-end with fake_dpi (+ ignored real-nDPI variant)

frontend/src/
├── api/types.ts                       # FlowRecord.app*, DpiStatus, RuleMatch.app*, AppCatalogEntry
├── components/AppTag.vue              # NEW (app name, dashed when inferred)
├── components/DpiStatusCard.vue       # NEW
├── components/RuleEditor.vue          # app / app_category fields with catalog picker
└── views/{FlowsView,HostDetailView,StatusView,DashboardView}.vue   # column+filter, top apps, card, by=app panel
frontend/tests/{unit/app.spec.ts, unit/dpi.spec.ts, e2e/status.spec.ts}

deploy/Dockerfile                      # + nDPI build stage and tmon-dpi target
docs/dpi.md                            # NEW: what/why, enable, sandbox, troubleshooting, LGPL obligations
docs/routeros.md                       # reply-direction rule
justfile                               # dpi-build, fixtures-apps, fuzz-dpi
```

**Structure Decision**:
- **Main workspace**: two new pure-Rust crates, `tmon-dpi-proto` (shared contract) and `tmon-dpi-client` (supervision and tap).
- **`backend/dpi/`**: a separate workspace for the sidecar and the C dependency.
- **Existing crates**: touched only at their seams: core types/rules, the capture tap, the pipeline step, storage columns, the API and config.

## Architecture & Data Flow

```text
Router ──TZSP──► capture listener ──► CaptureProcessor (feature 002 naming)
                                          │ after decode: PacketTap.try_send(key, dir, ts, L3 copy)   (skip DNS, final, >N pkts)
                                          ▼
                                 bounded queue (16k pkts / 16 MiB, drop+count)
                                          ▼
                          writer task ──SEQPACKET fd──►  tmon-dpi (sandboxed child, nDPI 6.0)
                          reader task ◄──────────────── Hello / Classification / Stats(heartbeat)
                              │                                   ▲
                              ├─► AppTable (sharded LRU, TTL)     │ Supervisor: spawn, handshake, 6 s heartbeat,
                              ├─► PendingApps ─(5 s)─► backfill_apps (15 min raw window)   back-off, failed
                              └─► DpiHealth (status, warnings, metrics)

Router ──IPFIX──► collector ──► Pipeline::enrich
                                  1–3. feature 001/002 (un-NAT, geo, naming)
                                  4. app = AppTable.classify(key)   → app, app_confident, app_category; host only if no 002 name
                                  5. rules (hostname 900k > app 860k/850k > ASN 800k > port 700k; higher wins) → confidence detected|inferred
                                  ▼
                               storage (app, app_confident, remote_host_source=dpi)
```

## Delivery Phases (input for /speckit-tasks)

1. **Setup**:
   - crates `tmon-dpi-proto` and `tmon-dpi-client`;
   - the `backend/dpi` workspace skeleton and `ndpi-sys` build;
   - `just dpi-build`;
   - fuzz scaffolding;
   - the fixture generator skeleton.
2. **Foundational**:
   - the IPC codec (contract tests first);
   - core `AppInfo`/trait and `FlowRecord` fields;
   - rule fields and `confidence_for`;
   - migration 7;
   - the `[dpi]` config;
   - the pipeline app step;
   - the capture tap seam with orientation;
   - the fake sidecar;
   - the supervisor skeleton (spawn, handshake).
3. **US1 (P1, MVP)**:
   - the sidecar classify loop with nDPI;
   - result mapping and the carrier filter;
   - `AppTable`, enrichment and back-fill;
   - built-in app rules and the File Sharing category;
   - app fixtures with real-nDPI tests;
   - end-to-end with the fake sidecar.
4. **US2 (P2)**:
   - API (`/flows` app field/filter, CSV, `by=app`, host top apps, catalog, rule fields);
   - UI (AppTag, Flows column/filter, drill-down, breakdown panel, rule editor picker).
5. **US3 (P2)**:
   - heartbeat, back-off and failed/incompatible/not-found states;
   - `/status.dpi`, warnings and metrics;
   - the DPI Status card.
6. **US4 (P3)**:
   - sandbox, allocator budget and `RLIMIT_AS`;
   - back-pressure and crash-loop tests;
   - IPC fuzzing (both directions plus sidecar);
   - the disabled-mode regression;
   - Docker image and licensing docs.
7. **Polish**:
   - benches (sidecar, tap, ingest);
   - memory checks;
   - code and security review;
   - docs (`dpi.md`, router reply rule, README);
   - quickstart validation (V11 on the real router).

## Complexity Tracking

| Violation | Why needed | Simpler alternative rejected because |
|---|---|---|
| Second process (`tmon-dpi`) with supervision | FR-002: C code parsing hostile input must not share fate with flow collection. FR-026: tmon must not link LGPL code | In-process FFI: one nDPI memory bug would crash ingest; it would also make tmon an LGPL "combined work" |
| Second Cargo workspace (`backend/dpi/`) | Keeps the C toolchain, nDPI build and bindgen out of the default build, CI and contributor setup | Workspace member behind a feature flag: `cargo test --workspace` and clippy `--all-targets` would still try to build it and need nDPI everywhere |
| Hand-specified binary IPC format | FR-027 (replaceable classifier) and FR-025 (fuzzable, bounded) | serde/postcard: Rust-specific, so not a public contract |
