---

description: "Task list for Application Classification with nDPI (optional sidecar)"
---

# Tasks: Application Classification with nDPI (Optional Sidecar)

**Input**: Design documents from `/specs/003-ndpi-app-classification/`

**Prerequisites**: plan.md, spec.md, research.md (R1–R14), data-model.md, contracts/dpi-ipc.md, contracts/http-api.md, contracts/config.md, contracts/router-capture.md, quickstart.md

**Tests**: INCLUDED. The plan's Constitution Check adopts TDD with ≥ 80 % coverage. In every phase, write the test tasks first and confirm they FAIL before starting the implementation tasks.

**Organization**: Tasks are grouped by user story, in priority order: US1 (P1), US2 (P2), US3 (P2), US4 (P3).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: User story from spec.md (US1–US4)
- Paths are relative to the repository root:
  - `backend/` is the main Cargo workspace (pure Rust, no nDPI);
  - `backend/dpi/` is the separate sidecar workspace (needs nDPI 6.0 and a C toolchain);
  - `frontend/` is the Vue SPA.
- Tasks that need the real nDPI are marked **(nDPI)**. Every other task must build and pass in the default CI without a C toolchain.

---

## Phase 1: Setup

**Purpose**: New crates, the sidecar workspace, the build of nDPI, and scaffolding

- [X] T001 Create the crates `backend/crates/tmon-dpi-proto` (pure Rust; depends only on `thiserror`) and `backend/crates/tmon-dpi-client` (depends on `tmon-core`, `tmon-capture`, `tmon-dpi-proto`, `tokio`). Each gets a `Cargo.toml` and a `src/lib.rs` with module stubs per plan.md. Add both to `members` in `backend/Cargo.toml`, and add `exclude = ["dpi"]` so `cargo test --workspace` never builds the sidecar
- [X] T002 Add the workspace dependencies to `backend/Cargo.toml`: `command-fds` and `nix` (features `socket`, `signal`, `process`), or the `rustix` already locked if present. Use them from `backend/crates/tmon-dpi-client/Cargo.toml`. Run `cargo tree -d` and confirm there are no new duplicate major versions
- [X] T003 Create the sidecar workspace `backend/dpi/Cargo.toml` with these members:
  - `ndpi-sys`: `build.rs` uses `pkg-config` (`libndpi >= 6.0`, honouring `NDPI_PREFIX`) and `bindgen`, allow-listing `ndpi_*` functions, types and constants.
  - `tmon-dpi`: a `[[bin]]` stub; path dependency on `../crates/tmon-dpi-proto`; dependencies `landlock`, `seccompiler`, `libc`.

  Give it its own `rust-toolchain`/lint settings matching the main workspace and its own `Cargo.lock`
- [X] T004 Add `justfile` recipes:
  - `dpi-build`: clones nDPI tag `6.0` into `backend/dpi/target/ndpi-src`, runs `./autogen.sh && ./configure --with-only-libndpi --prefix=$PWD/backend/dpi/target/ndpi && make install`, then `cargo build --release --manifest-path backend/dpi/Cargo.toml` with `PKG_CONFIG_PATH` set.
  - `fixtures-apps`: runs the app fixture generator.
  - `fuzz-dpi SECS`: both codec targets plus `sidecar_packets`.
  - `test-dpi`: the sidecar workspace tests.
- [X] T005 [P] Add the fuzz target stubs `dpi_proto_to_sidecar` and `dpi_proto_to_tmon` to `backend/fuzz/Cargo.toml` and `backend/fuzz/fuzz_targets/*.rs` (they compile but are empty). Create the skeleton `backend/dpi/fuzz/` (cargo-fuzz layout) with an empty `fuzz_targets/sidecar_packets.rs`
- [X] T006 [P] Add the module skeleton `backend/crates/tmon-capture/src/fixtures/apps.rs` (behind the existing `fixtures` feature) and the example `backend/crates/tmon-capture/examples/gen_app_fixtures.rs` (`required-features = ["fixtures"]`). The example writes `backend/tests/fixtures/capture/apps_*.pcap` and the matching `apps_flows.pcap`
- [X] T007 [P] Add the example skeleton `backend/crates/tmon-dpi-client/examples/fake_dpi.rs`, a pure-Rust fake classifier. It reads fd 3 and parses `--mode classify|crash_after=N|hang_after=N|slow_us=N|garbage|silent` and `--proto-version N`. It compiles but does nothing yet

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The protocol, core types, rule fields, storage columns, config, the pipeline app step, the capture tap seam with correct orientation, the fake classifier and a supervisor that can spawn and handshake. After this phase, tmon can start the fake classifier and attach its answers to flows. Disabled mode is identical to feature 002.

### Tests first

- [X] T008 [P] Codec contract tests in `backend/crates/tmon-dpi-proto/tests/codec.rs`, following contracts/dpi-ipc.md:
  - Round-trip every message type: 0x01 Packet, 0x02 Forget, 0x03 Ping, 0x81 Hello, 0x82 Classification, 0x83 Stats, 0x84 HelloCatalog, 0x85 Pong.
  - Rejected with a typed error (never a panic):
    - a message longer than 9,400 bytes;
    - an unknown type;
    - a truncated header or body;
    - length fields that point past the end;
    - counts above the contract limits;
    - non-printable-ASCII names, and names longer than 48 bytes;
    - trailing bytes.
  - `proptest`: encode → decode is the identity, and decode of arbitrary bytes never panics.
- [X] T009 [P] Core tests in `backend/crates/tmon-core/tests/app.rs`:
  - `AppInfo::new` validates "1–48 printable ASCII" for `app` and `category` (an empty category is allowed) and rejects the `GENERIC_CARRIERS` (`TLS`, `QUIC`, `HTTP`, `DNS`, `Unknown`, matched case-insensitively).
  - `host` goes through `normalize_host`; an invalid host becomes `None`.
  - `better_than`: confident beats guess, a guess never replaces a confident result (FR-006), and the same app is not "better".
  - `NameSource::Dpi` round-trips code `6` and the string `"dpi"`, and has the lowest priority.
  - `FlowRecord::with_app` returns a new record and leaves the input unchanged.
- [X] T010 [P] Rule tests in `backend/crates/tmon-core/tests/rules_app.rs`:
  - `app` and `app_category` match case-insensitively.
  - A flow without an app matches neither.
  - Validation rejects 0 or more than 64 entries, and entries of 0 or more than 48 characters (`validation_failed` with the field name).
  - `confidence_for`: hostname → Detected; app matched with `app_confident` → Detected; app matched without it → Inferred; ASN/CIDR/port unchanged.
  - The app candidate index returns the same result as a linear scan over 1,000 random rules.
- [X] T011 [P] Storage tests in `backend/crates/tmon-store/tests/migrations.rs` (extend):
  - Migration 7 adds nullable `app VARCHAR` and `app_confident BOOLEAN` to `flows_hot` and `batch_stage`.
  - `max(version)` is 7. Update the assertion in `backend/crates/tmon-store/tests/writer.rs`.
  - A Parquet file written without the columns still reads through the `flows` view and through recategorize rewrite/rebuild, with the values NULL.
  - Ingest round-trips `app`/`app_confident`; with no app, `app_confident` is stored as NULL.
- [X] T012 [P] Config tests in `backend/crates/tmon/tests/config.rs` (extend), following contracts/config.md:
  - the defaults;
  - the env override `TMON_DPI__ENABLED`;
  - `dpi.enabled = true` with `capture.enabled = false` → `dpi.enabled: requires capture.enabled = true`;
  - each numeric key just outside its range → `dpi.<key>: must be between A and B`;
  - an empty `binary` is rejected;
  - a non-existent binary path is **not** a config error.
- [X] T013 [P] Pipeline test in `backend/crates/tmon-enrich/tests/pipeline_app.rs`, with a stub `ConnectionClassifier`, following data-model.md §2:
  1. A feature 002 name is kept.
  2. Without one, `AppInfo.host` is used with `remote_host_source = Dpi`.
  3. `app`, `app_confident` and `app_category` are set.
  4. Categorization sees `app_category`.

  With no classifier configured, the output is byte-identical to feature 002.
- [X] T014 [P] Capture tap tests in `backend/crates/tmon-capture/tests/tap.rs`. Cover **reply orientation**: a WAN → LAN packet (server reply, research R5) produces the same LAN-side `ConnKey` as the LAN → WAN packet of that connection, decided by `LocalNetworks`, not by "source = client". Also check:
  - Feature 002 naming ignores the reply packets (no new names, no change in counters except `datagrams`).
  - The tap receives a copy of the L3 packet with `dir`, `ts_ms` and the oriented key.
  - The tap skips port 53 and keys in the final set, and stops after `max_packets_per_conn` packets per key.
  - A full tap never blocks the processor; the drop is counted.
  - Use a recording `PacketTap` stub.
- [X] T015 [P] Supervisor handshake tests in `backend/crates/tmon-dpi-client/tests/handshake.rs`, using `fake_dpi`:
  - A valid Hello → `Running`, with the catalog stored.
  - `--proto-version 2` → `Incompatible` with both versions in the error, and **no** restart.
  - Missing binary → `NotFound`.
  - No Hello within 5 s (`--mode silent`) → the child is killed and the failure counted.
  - The child gets a clean environment and only fd 3 besides stdio.

### Implementation

- [X] T016 Implement the codec in `backend/crates/tmon-dpi-proto/src/{lib,header,to_sidecar,to_tmon,limits}.rs`, exactly as in contracts/dpi-ipc.md. It is shared by tmon, the fake classifier and the sidecar. Allocation is bounded by the message size, and no message ever panics (makes T008 pass)
- [X] T017 [P] Implement `backend/crates/tmon-core/src/app.rs`: `AppInfo` (constructor with validation, `better_than`), `GENERIC_CARRIERS`, the `ConnectionClassifier` trait, and `validate_app_name`. Export them from `lib.rs`
- [X] T018 Add `NameSource::Dpi` (code 6) in `backend/crates/tmon-core/src/naming.rs`. Add `app: Option<Arc<str>>`, `app_confident: bool` and `app_category: Option<Arc<str>>` (`#[serde(skip)]`) to `backend/crates/tmon-core/src/flow.rs`, with the builder `with_app(&AppInfo) -> FlowRecord`. Update the constructors and fixtures that build `FlowRecord` (depends on T017)
- [X] T019 Rules in `backend/crates/tmon-core/src/rules/{model,matcher,confidence,compile}.rs`:
  - `RuleMatch.app` and `RuleMatch.app_category` ("1–64 entries, each 1–48 printable ASCII, compared case-insensitively");
  - matching against `flow.app` and `flow.app_category`;
  - the app-name candidate index beside the hostname index;
  - `confidence()` replaced by `confidence_for(&self, flow)`, with its single call site updated.

  Makes T010 pass (depends on T018)
- [X] T020 Storage:
  - migration 7 in `backend/crates/tmon-store/src/duck/schema.rs`;
  - `app`, `app_confident` appended to `FLOW_COLUMNS` and `ADDED_FLOW_COLUMNS`;
  - writes in `ingest.rs` and reads in `rows.rs`;
  - `remote_host_source` code 6 ↔ `dpi` in the row mapping.

  Makes T011 pass (depends on T018)
- [X] T021 [P] Config in `backend/crates/tmon/src/config_dpi.rs`: the `DpiConfig` serde struct with the defaults and ranges from contracts/config.md, `validate(&CaptureConfig)`, and conversion to `tmon_dpi_client::DpiSettings`. Register it in `config.rs` (makes T012 pass)
- [X] T022 Pipeline in `backend/crates/tmon-enrich/src/pipeline.rs`: an optional `Arc<dyn ConnectionClassifier>` set with `with_classifier`, and the app step after naming as in data-model.md §2. Keep the function under 50 lines by extracting `apply_app` (makes T013 pass; depends on T018, T019)
- [X] T023 Capture seam in `backend/crates/tmon-capture`:
  - a trait `PacketTap: Send + Sync { fn offer(&self, key: &ConnKey, dir: Dir, ts_ms: i64, l3: &[u8]) }` in `src/tap.rs`;
  - `CaptureProcessor` takes an `Option<Arc<dyn PacketTap>>` and `LocalNetworks`;
  - in `src/processor/mod.rs`, connection keys are oriented by `LocalNetworks`, and segments from the server side bypass the 002 naming extractors;
  - after decode, the processor calls `tap.offer` (skip rules live in the tap implementation, except port 53).

  Re-run the feature 002 suites to prove no change in naming (makes T014 pass)
- [X] T024 [P] Fill in `backend/crates/tmon-dpi-client/examples/fake_dpi.rs`:
  - Hello (with a small catalog) within 100 ms.
  - Stats every 2 s.
  - Classify by destination port: 6881 → BitTorrent/`Download` confident final; 51820 → WireGuard/VPN guess; 27015 → Steam/Game confident; 3389 → RDP/RemoteAccess confident.
  - The modes from T007: `crash_after` exits with code 101, `hang_after` stops reading, `slow_us` sleeps per packet, `garbage` sends random bytes, `silent` never sends Hello.
  - It uses `tmon-dpi-proto`.
- [X] T025 Supervisor skeleton in `backend/crates/tmon-dpi-client/src/{settings,spawn,supervisor}.rs`:
  - `DpiSettings` and the `SupervisorState` enum `Disabled | NotFound | Starting | Running | Restarting | Failed | Incompatible`.
  - `spawn`: a `SOCK_SEQPACKET` socketpair (CLOEXEC on tmon's end), the child end passed as fd 3 via `command-fds`, `env_clear()`, args from the settings, `kill_on_drop(true)`.
  - The binary is resolved via `PATH` or as an absolute path; when missing → `NotFound`.
  - Handshake: a 5 s Hello timeout and a `proto_version == 1` check.
  - The state is exposed through `watch::Receiver`.

  Makes T015 pass (depends on T016, T024)
- [X] T026 Wire it in `backend/crates/tmon/src/dpi.rs` and `backend/crates/tmon/src/app.rs`. When `[dpi]` is absent or disabled, nothing is created and no process is spawned (SC-007). When enabled, tmon:
  - builds the `DpiHandle` (supervisor, tap, AppTable);
  - passes the tap to the capture processor and the classifier to the pipeline;
  - shuts down cleanly on SIGTERM (the child is killed, and the socket is closed first).

**Checkpoint**: `cargo test --workspace` is green. Feature 001 and 002 suites are unchanged. With the fake classifier, tmon starts it and reaches `Running`.

---

## Phase 3: User Story 1 — Recognize applications that announce no name (P1) 🎯 MVP

**Goal**: BitTorrent, WireGuard, Steam, call and remote-desktop connections get an application and the right category, with "detected" confidence.

**Independent test**: Replay `apps_mix.pcap` + `apps_flows.pcap` with the real sidecar (quickstart V2–V4). In CI, run `backend/tests/us8_dpi.rs` with the fake classifier.

### Tests first

- [X] T027 [P] [US1] Unit tests in `backend/dpi/tmon-dpi/src/classify.rs` (`#[cfg(test)]`, pure Rust over a plain `NdpiResult` struct, so they run without nDPI):
  - Confidence mapping per research R6: `DPI`, `DPI_CACHE`, `CUSTOM_RULE` → confident; `DPI_PARTIAL`, `DPI_PARTIAL_CACHE`, `MATCH_BY_IP`, `MATCH_BY_PORT`, `DPI_AGGRESSIVE` → guess; `UNKNOWN` → none.
  - The app protocol is preferred. The master protocol is used only when the app protocol is unknown and the master is not a generic carrier.
  - Generic carriers are never reported.
- [X] T028 [P] [US1] Unit tests in `backend/dpi/tmon-dpi/tests/flows.rs`, with a mock engine trait:
  - `DpiFlow` LRU at `max_flows`, counted as `evicted{lru}`.
  - Idle sweep after `idle_timeout`, counted as `evicted{idle}`.
  - A Classification is sent only when `(app, confident)` changes.
  - `final=true` once confident or after giving up at `max_packets_per_conn`; the flow is freed after `final`.
  - `Forget` frees the flow.
- [ ] T029 [P] [US1] (nDPI) Integration test `backend/dpi/tmon-dpi/tests/apps.rs`. It spawns the built `tmon-dpi` over a socketpair and replays the packets of each `apps_*` fixture through the codec. Expected: — *code written and typechecked; run after `just dpi-build`*
  - BitTorrent, WireGuard, OpenVPN, STUN/RTP, Steam and RDP are identified as expected with confident results: ≥ 90 % of fixture connections (SC-002).
  - The Zoom TLS connection reports Zoom.
  - The UDP/51820 non-WireGuard payload is not reported as a confident WireGuard result.
- [X] T030 [P] [US1] `AppTable` tests in `backend/crates/tmon-dpi-client/tests/table.rs`:
  - Inserts follow `better_than` (a guess does not replace a confident result).
  - TTL = `idle_timeout`; `classify` refreshes `last_used`.
  - Capacity `max_flows × 2` with eviction counted.
  - Sharding is thread-safe under 8 concurrent writers.
- [X] T031 [P] [US1] I/O tests in `backend/crates/tmon-dpi-client/tests/io.rs`, with `fake_dpi --mode classify`:
  - Packets offered to the tap reach the fake classifier.
  - Classifications land in `AppTable` and in `PendingApps`.
  - A `final` result adds the key to the final set, and further packets for it are skipped (`skipped_final`).
  - When the classifier is not `Running`, offers count `dropped_down`.
- [X] T032 [P] [US1] Back-fill tests in `backend/crates/tmon-store/tests/backfill_apps.rs`. Rows in `[now − 15 min, now]` with matching 5-tuple and NULL `app` get `app`/`app_confident` and are re-categorized. These are left alone:
  - rows outside the window;
  - rows with an app already;
  - non-matching tuples.

  At most 10k per drain. This mirrors the feature 002 `backfill_hostnames` tests.
- [X] T033 [P] [US1] Built-in rule tests in `backend/crates/tmon-core/tests/builtin_rules.rs` (extend):
  - The category `File Sharing` (id 15) exists.
  - BitTorrent → File Sharing, WireGuard → VPN/Tunnels, Steam → Gaming, Zoom → Messaging/VoIP, RDP → Remote Access, each Detected when confident and Inferred when guessed.
  - An nDPI-category fallback (`VPN`) applies when no named-app rule matches.
  - A hostname rule (e.g. `netflix.com`) beats an app rule on the same flow.
  - A user rule `app = [Zoom] → Work` (priority ≥ 1,000,000) beats the built-in rule (US1 scenario 5).
  - All app rules are in 850,000–869,999.
- [X] T034 [P] [US1] End-to-end test `backend/tests/us8_dpi.rs`: tmon with `[capture]` + `[dpi] binary = <fake_dpi>`. Replay the app fixture capture to the TZSP port and the flows to the collector. Expected:
  - The stored flows carry `app`, `app_confident` and the expected category/confidence.
  - A Classification that arrives after the flow is stored is back-filled within 10 s.
  - `remote_host_source = dpi` appears only when 002 has no name.

  Add an `#[ignore]` variant that uses the real `tmon-dpi` when `TMON_DPI_BIN` is set

### Implementation

- [X] T035 [P] [US1] App fixture generators in `backend/crates/tmon-capture/src/fixtures/apps.rs`, per research R12. Each produces both directions:
  - BitTorrent BEP 3 handshake;
  - WireGuard initiation/response (type 1/2, 148/92 bytes);
  - OpenVPN `P_CONTROL_HARD_RESET_CLIENT_V2`;
  - STUN binding request/response (RFC 8489 magic cookie) then RTP;
  - Steam A2S_INFO query/response;
  - RDP X.224 with `Cookie: mstshash=`;
  - TLS ClientHello SNI `zoom.us`;
  - UDP/51820 with a random payload.

  Finish `examples/gen_app_fixtures.rs` to write `apps_mix.pcap`, `apps_zoom_tls.pcap`, `apps_port_only.pcap` and `apps_flows.pcap`. Keep each file ≤ 400 lines (split into `apps/` submodules if needed)
- [ ] T036 [US1] (nDPI) The safe wrapper in `backend/dpi/tmon-dpi/src/ndpi.rs`, the **only** module with `unsafe`: — *code written and typechecked; run after `just dpi-build`*
  - `Module` RAII: `ndpi_init_detection_module` + `ndpi_finalize_initialization`; `Drop` → `ndpi_exit_detection_module`.
  - `Flow` RAII: allocated with `ndpi_detection_get_sizeof_ndpi_flow_struct`; `Drop` → `ndpi_flow_free`.
  - `process(&mut Flow, l3, ts_ms) -> NdpiResult` via `ndpi_detection_process_packet`.
  - `giveup(&mut Flow)` via `ndpi_detection_giveup`.
  - `proto_name`, `category_name`, `confidence`, `version()` (`ndpi_revision`, `ndpi_get_api_version`).
  - `catalog()`, which iterates the protocol ids.

  Every `unsafe` block gets a `// SAFETY:` comment
- [X] T037 [US1] Result mapping in `backend/dpi/tmon-dpi/src/classify.rs` (makes T027 pass): `NdpiResult → Option<Classification>` with the carrier filter and the confidence map
- [X] T038 [US1] Flow table in `backend/dpi/tmon-dpi/src/flows.rs` (makes T028 pass). It is generic over an `Engine` trait implemented by `ndpi::Module`, and handles:
  - the LRU of `DpiFlow` and the idle sweep;
  - "report on change";
  - `final` handling and `Forget`;
  - the counters from data-model.md §4.
- [X] T039 [US1] Sidecar I/O loop and entry point in `backend/dpi/tmon-dpi/src/{ipc,main}.rs`:
  - parse the args (the same keys as `DpiSettings`);
  - initialize nDPI;
  - send Hello and HelloCatalog;
  - loop: `recvmsg` on fd 3 → decode → `flows.process` → send a Classification on change;
  - send Stats every 2 s, answer Ping with Pong;
  - exit 0 on EOF.

  Invalid messages are counted (`packets_invalid`) and never fatal. No payload is logged (FR-024)
- [X] T040 [P] [US1] `AppTable` in `backend/crates/tmon-dpi-client/src/table.rs`: sharded LRU, TTL, `better_than` inserts, and `impl ConnectionClassifier` (makes T030 pass)
- [X] T041 [P] [US1] `DpiTap` in `backend/crates/tmon-dpi-client/src/tap.rs`, implementing `PacketTap`:
  - a bounded `mpsc` (`queue_capacity`) with a byte budget (`queue_max_bytes`, an `AtomicUsize` as in feature 002);
  - the final-set LRU (100k);
  - the per-key packet counter LRU capped at `max_packets_per_conn`;
  - counters `sent`, `dropped_queue`, `dropped_down`, `skipped_final`;
  - `try_send` only, never `.await`.
- [X] T042 [US1] I/O tasks in `backend/crates/tmon-dpi-client/src/io.rs`:
  - The writer drains the tap queue into the socket, encoding with `tmon-dpi-proto`.
  - The reader decodes each message:
    - Classification → `AppInfo::new` (invalid → `app_invalid`) → `AppTable` + `PendingApps` + the final set;
    - Stats → the last stats and the heartbeat time;
    - HelloCatalog → the catalog.
  - Both end on socket error and report to the supervisor.

  Makes T031 pass (depends on T040, T041)
- [X] T043 [US1] Back-fill:
  - `backend/crates/tmon-dpi-client/src/pending.rs`: `PendingApps`, at most 10k per drain, overflow counted;
  - `backend/crates/tmon-store/src/duck/backfill_apps.rs`: the same mechanics as `backfill.rs`, with the shared helpers reused rather than copied;
  - a 5 s drain task in `backend/crates/tmon/src/dpi.rs`.

  Makes T032 pass
- [X] T044 [P] [US1] Built-in data in `backend/assets/builtin-rules.toml`:
  - the category `File Sharing` (id 15, with colour and icon like the others);
  - about 25 rules per research R10: named apps in 860,000–869,999 and nDPI categories in 850,000–859,999.

  Makes T033 pass
- [X] T045 [US1] Connect everything in `backend/crates/tmon/src/dpi.rs`: supervisor ↔ I/O tasks ↔ AppTable ↔ pipeline classifier ↔ back-fill drain. Make the US1 end-to-end test T034 pass with the fake classifier, then run the `#[ignore]` real-nDPI variant locally after `just dpi-build`

**Checkpoint**: quickstart V2–V4 pass with the real classifier. This is the MVP.

---

## Phase 4: User Story 2 — See and use applications in the UI (P2)

**Goal**: The app column and filter in Flows, CSV export, the host drill-down's top apps, an app breakdown on the Dashboard, and app fields in the rule editor.

**Independent test**: quickstart V5.

### Tests first

- [X] T046 [P] [US2] API tests in `backend/crates/tmon-api/tests/flows_app.rs`:
  - `GET /flows?app=BitTorrent&app=wireguard` matches exactly and case-insensitively.
  - Rows include `app` and `app_confident`.
  - `remote_host_source` may be `dpi`.
  - The `app` filter on a non-raw range → `unsupported_filter_range`.
  - The CSV has the columns `app,app_confident` right after `remote_host_source`, empty for unclassified flows.
- [X] T047 [P] [US2] API tests in `backend/crates/tmon-api/tests/breakdown_app.rs`:
  - `by=app` returns `{key, bytes, flows, confident_share}` with `key: null` for flows without an app, and respects the `host`/`category`/`direction` filters.
  - A range beyond raw retention → `unsupported_filter_range`.
  - `GET /hosts/{ip}` contains `top_apps` (≤ 10, empty for a non-raw range).
  - `GET /applications/catalog` returns `[]` when DPI is disabled, and the sorted catalog when running.
- [X] T048 [P] [US2] Rules API tests in `backend/crates/tmon-api/tests/rules.rs` (extend):
  - Create/update with `app`/`app_category` works.
  - These are rejected with `validation_failed` and the field error: 0 entries, 65 entries, a 49-character name, non-ASCII.
  - `POST /rules/test` reports `confidence` `detected` or `inferred` per matched flow, depending on `app_confident`.
  - "Re-apply to history" categorizes by the stored `app`.
- [X] T049 [P] [US2] Frontend unit tests in `frontend/tests/unit/app.spec.ts`:
  - `AppTag` renders the name, has a dashed style plus "guessed by classifier" title when not confident, and renders nothing for null.
  - The FlowsView app column and filter control update the query (`app=` param).
  - HostDetailView lists `top_apps`.
  - The `ApplicationsTable` toggle "By rule / By app" calls `by=app`.
  - The RuleEditor app field offers the catalog (search-as-you-type) with free-text fallback, and validates 1–48 characters.
- [X] T050 [P] [US2] Playwright in `frontend/tests/e2e/flows.spec.ts` (extend or create): with the mocked API, filter by app and see only those rows. Export the CSV and check the header contains `app`

### Implementation

- [X] T051 [US2] Queries in `backend/crates/tmon-store/src/duck/query/flows.rs`: the `app` filter (`lower(app) IN (…)`, parameterized, raw only) and the `app`/`app_confident` selection and CSV columns (makes the storage half of T046 pass)
- [X] T052 [US2] Queries in `backend/crates/tmon-store/src/duck/query/traffic.rs` (`by=app` with `confident_share`) and `backend/crates/tmon-store/src/duck/query/host.rs` (`top_apps`, ≤ 10). Both are raw only and return `unsupported_filter_range` otherwise
- [X] T053 [US2] API in `backend/crates/tmon-api/src`:
  - the `app` param parsing in `params.rs` (each 1–48 printable ASCII, ≤ 64 values);
  - the `routes/flows.rs`, `routes/traffic.rs` and `routes/hosts.rs` additions;
  - the new `routes/applications.rs` (`/applications/catalog` from `DpiHandle`, or `[]`);
  - `openapi.rs` updated (the app fields and filters, `by=app`, the catalog). The route-scan test stays green.

  Makes T046, T047 pass
- [X] T054 [US2] Rule API validation in `backend/crates/tmon-api/src/routes/rules.rs`, plus `confidence_for` in the rule-test response (makes T048 pass)
- [X] T055 [P] [US2] Types in `frontend/src/api/types.ts`:
  - `FlowRecord.app`, `FlowRecord.app_confident`;
  - `'dpi'` in `NAME_SOURCES`;
  - `RuleMatch.app`, `RuleMatch.app_category`;
  - `AppCatalogEntry`, `HostDetail.top_apps`.

  Add `NAME_SOURCE_INFO.dpi` in `frontend/src/constants.ts`
- [X] T056 [P] [US2] Component `frontend/src/components/AppTag.vue` (`data-testid="app-tag"`, `data-confident`)
- [X] T057 [US2] `frontend/src/views/FlowsView.vue` and `frontend/src/components/FilterBar.vue`: the app column (AppTag) and the app filter (multi-select fed by `/applications/catalog`, free text allowed)
- [X] T058 [P] [US2] `frontend/src/views/HostDetailView.vue`: the "Top applications" list (hidden when empty)
- [X] T059 [P] [US2] `frontend/src/components/ApplicationsTable.vue` and `frontend/src/views/DashboardView.vue`: a "By rule / By app" toggle. The by-app mode calls `by=app` and shows `confident_share`. It shows a hint "requires application classification" when DPI is disabled
- [X] T060 [US2] `frontend/src/components/RuleEditor.vue` and `frontend/src/components/RuleTestPanel.vue`: the `app` and `app_category` fields with catalog search, and confidence shown per test match (makes T049, T050 pass)

**Checkpoint**: quickstart V5 passes, and `pnpm test:unit` + `pnpm test:e2e` are green.

---

## Phase 5: User Story 3 — Know the classifier is healthy (P2)

**Goal**: The supervisor restarts with back-off and fails after repeated crashes. Status card, warnings and metrics.

**Independent test**: quickstart V6–V8.

### Tests first

- [X] T061 [P] [US3] Supervisor tests in `backend/crates/tmon-dpi-client/tests/supervisor.rs`, using `fake_dpi` and paused tokio time where possible:
  - `crash_after=N` → `Restarting`, then `Running`, with `restarts_total` incremented.
  - The back-off sequence is 1, 2, 4, … 60 s.
  - 5 failures within 10 min → `Failed`, with no further spawns.
  - `hang_after=N` → no Stats for 6 s → SIGKILL, then restart.
  - `garbage` → `ipc_invalid` counted; 100 invalid messages in 10 s → restart.
  - Flow ingest continues throughout: the pipeline is not blocked and 0 flows are dropped.
- [X] T062 [P] [US3] Health tests in `backend/crates/tmon-dpi-client/tests/health.rs`:
  - The `DpiStatus` snapshot fields follow contracts/http-api.md.
  - The warnings `dpi_not_found`, `dpi_incompatible` (both versions named), `dpi_failed` (last error), `dpi_restarting` (≥ 2 in 10 min, not failed) and `dpi_dropping` (drop share > 5 % over a 15 min window). Each has a remediation hint.
  - No warnings when disabled.
- [X] T063 [P] [US3] API tests in `backend/crates/tmon-api/tests/status_dpi.rs`:
  - The `/status` `dpi` key is always present: `{"enabled": false}` when disabled, the full object when enabled.
  - The warnings appear in `warnings[]`.
  - `/metrics` exposes the `tmon_dpi_*` series from contracts/http-api.md with bounded labels (no per-app label).
- [X] T064 [P] [US3] Frontend tests:
  - `frontend/tests/unit/dpi.spec.ts`: `DpiStatusCard`, with testids `dpi-card`, `dpi-state`, `dpi-engine`, `dpi-uptime`, `dpi-packets`, `dpi-classified`, `dpi-drops`, `dpi-restarts`, `dpi-top-apps`. It renders each state, and the drops are highlighted above 5 %.
  - The warning guides for each `dpi_*` code.
  - Extend `frontend/tests/e2e/status.spec.ts` with the card.

### Implementation

- [X] T065 [US3] Supervisor loop in `backend/crates/tmon-dpi-client/src/supervisor.rs`:
  - heartbeat watchdog (6 s);
  - exponential back-off capped at 60 s;
  - failure ring (5 in 10 min → `Failed`);
  - invalid-message rate trigger;
  - SIGKILL + reap on restart;
  - a clean shutdown path.

  Split it into `supervisor/{mod,backoff,watchdog}.rs` if it exceeds 400 lines (makes T061 pass)
- [X] T066 [US3] Health in `backend/crates/tmon-dpi-client/src/health.rs`: the counters, a 15-min drop-share window (per-minute buckets), the `DpiStatus` snapshot (includes classifier Stats, `top_apps`), and the `DpiWarning` enum with `code()` and `message()` (makes T062 pass)
- [X] T067 [US3] API:
  - `backend/crates/tmon-api/src/routes/dpi.rs` (the `DpiStatus` serialization, untagged enabled/disabled as for capture), merged into `routes/status.rs`;
  - `warnings.rs` maps the `DpiWarning`s;
  - the new `metrics_dpi.rs`;
  - the `openapi.rs` `DpiStatus` schema and warning codes.

  Makes T063 pass
- [X] T068 [US3] Frontend:
  - `frontend/src/components/DpiStatusCard.vue` in `frontend/src/views/StatusView.vue` (beside CaptureStatusCard);
  - in `frontend/src/constants.ts`: `WARNING_GUIDES` for the `dpi_*` codes (links to `docs/dpi.md#troubleshooting`) and `DPI_DROP_SHARE_WARN = 0.05`;
  - `DpiStatus` types in `frontend/src/api/types.ts`.

  Makes T064 pass

**Checkpoint**: quickstart V6, V7 and V8 pass with the fake classifier.

---

## Phase 6: User Story 4 — Safe, isolated and optional (P3)

**Goal**: Sandbox, memory bounds, back-pressure, fuzzing, disabled-mode equivalence, packaging and licensing.

**Independent test**: quickstart V1, V9 and V10.

### Tests first

- [ ] T069 [P] [US4] (nDPI) Sandbox tests in `backend/dpi/tmon-dpi/tests/sandbox.rs`: — *code written and typechecked; run after `just dpi-build`*
  - The spawned sidecar's `/proc/<pid>/status` shows `NoNewPrivs: 1` and `Seccomp: 2`.
  - A test-only `--self-test-network` flag makes the child call `socket()` after sandboxing: the child dies with SIGSYS and the parent sees it.
  - `--self-test-open` with `open("/etc/passwd")` is denied.
  - The startup log lists each restriction as applied or unavailable.
- [ ] T070 [P] [US4] (nDPI) Memory tests in `backend/dpi/tmon-dpi/tests/memory.rs`. With `memory_limit_mb = 32` and 200k random flows: — *code written and typechecked; run after `just dpi-build`*
  - live bytes stay ≤ the budget;
  - `evicted{memory}` > 0;
  - the process stays alive;
  - RSS < limit + 64 MB.
- [X] T071 [P] [US4] Back-pressure tests in `backend/crates/tmon-dpi-client/tests/backpressure.rs`:
  - `fake_dpi --mode slow_us=2000` with a 50k-packet burst: the queue never exceeds `queue_capacity` or `queue_max_bytes`; `dropped_queue` > 0; the offer latency is p99 < 5 µs; the capture processor is never blocked.
  - A flow ingest of 10k records running at the same time drops 0.
- [X] T072 [P] [US4] Disabled-mode regression in `backend/tests/us8_dpi.rs` (extend):
  - With no `[dpi]` section: no child process exists (check `/proc` for children), `/status.dpi = {"enabled": false}`, and the flows are identical to a feature 002 run on the same fixtures.
  - With `binary = "/nonexistent"`: tmon serves normally, and the `dpi_not_found` warning is raised.

### Implementation

- [ ] T073 [US4] (nDPI) Counting allocator in `backend/dpi/tmon-dpi/src/alloc.rs`: — *code written and typechecked; run after `just dpi-build`*
  - `malloc`/`calloc`/`realloc`/`free` wrappers with a size header and an `AtomicUsize` of live bytes, registered via `ndpi_set_memory_alloction_functions` **before** `ndpi_init_detection_module`;
  - budget enforcement in `flows.rs`: above the limit, give up and free LRU flows until under 90 %, counted as `evicted{memory}`.

  Makes T070 pass
- [ ] T074 [US4] (nDPI) Sandbox in `backend/dpi/tmon-dpi/src/sandbox.rs`, applied after nDPI initialization, in order: — *code written and typechecked; run after `just dpi-build`*
  1. `PR_SET_NO_NEW_PRIVS`;
  2. if running as root: `setgroups([])`, then `setgid`/`setuid` to `nobody`;
  3. Landlock deny-all (best effort);
  4. a seccompiler allow-list per research R8, with `SECCOMP_RET_KILL_PROCESS` otherwise;
  5. `RLIMIT_AS = memory_limit_mb + 64 MB`.

  Also `PR_SET_PDEATHSIG(SIGKILL)` first thing in `main`. Each step is logged. Makes T069 pass
- [X] T075 [P] [US4] Fill in the fuzz targets `backend/fuzz/fuzz_targets/dpi_proto_to_sidecar.rs` and `dpi_proto_to_tmon.rs`: decode arbitrary bytes, and re-encode/decode round-trip anything that decodes. Add the corpus seeds from the T008 vectors to `backend/fuzz/corpus/` via the existing seed generator
- [ ] T076 [P] [US4] (nDPI) Fill in `backend/dpi/fuzz/fuzz_targets/sidecar_packets.rs`: arbitrary bytes → a sequence of Packet messages → the `flows` table with the real nDPI (ASan build, `memory_limit_mb = 64`). Add the seeds from the `apps_*` fixtures — *code written and typechecked; run after `just dpi-build`*
- [X] T077 [US4] Packaging in `deploy/Dockerfile`:
  - an `ndpi-build` stage (pinned nDPI 6.0 tag, `--with-only-libndpi`);
  - a `dpi-build` stage (`cargo build --release --manifest-path backend/dpi/Cargo.toml`);
  - a target `tmon-dpi` = the base runtime + `/usr/local/bin/tmon-dpi` + `/usr/local/lib/libndpi.so.6` (+ `ldconfig`) + `/usr/share/doc/tmon-dpi/` (nDPI `COPYING`, a written source offer, the tag and the source URL);
  - the base image unchanged.

  Update `.dockerignore` and the `justfile` (`docker-dpi`). Confirm the base image contains no `libndpi`
- [X] T078 [US4] Documentation: create `docs/dpi.md`. It covers:
  - what the feature does and why it is optional;
  - how to enable it (native or the `-dpi` image);
  - the router reply-direction rule;
  - the sandbox layers and how to verify them;
  - memory sizing;
  - troubleshooting for each `dpi_*` warning;
  - **licensing**: nDPI is LGPL-3.0, linked dynamically; redistributors must ship the license and the source or a written offer, allow relinking, and not link statically without object files; tmon itself remains MIT;
  - the IPC protocol link (`specs/003-ndpi-app-classification/contracts/dpi-ipc.md`), for alternative classifiers (FR-027).

**Checkpoint**: quickstart V1, V9 and V10 pass.

---

## Phase 7: Polish & Cross-Cutting Concerns

- [X] T079 [P] Benches:
  - `backend/dpi/tmon-dpi/benches/classify.rs` (nDPI): the fixture mix must reach ≥ 20k packets/s on one core.
  - Extend `backend/crates/tmon-capture/benches/capture_pipeline.rs` with a `DpiTap`: it must stay ≥ 50k datagrams/s.
  - Add `DPI=1` (start `fake_dpi`, or the real sidecar if `TMON_DPI_BIN` is set) to `scripts/bench-ingest.sh`, and confirm ingest is within 5 % of feature 002 (SC-003).
- [ ] T080 [P] Measure memory: — *partial: see validation.md (nDPI and Docker steps pending)*
  - tmon RSS with DPI on at defaults, under the V8 flood: ≤ the feature 002 bound + 16 MB;
  - sidecar RSS after 10 min of flood: < 256 MB (SC-005).

  Record both in `specs/003-ndpi-app-classification/validation.md`
- [X] T081 [P] Code review of `tmon-dpi-proto`, `tmon-dpi-client` and `backend/dpi` (code-reviewer and security-reviewer agents). Focus on:
  - `unsafe` in `ndpi.rs`/`alloc.rs`;
  - fd handling;
  - the sandbox ordering;
  - decode limits.

  Address CRITICAL/HIGH findings. Check that files stay ≤ 400 lines, functions < 50 lines, and `unsafe` appears only in `ndpi.rs` and `alloc.rs`
- [X] T082 Run the gates:
  - main workspace: `cargo fmt --check`, `cargo lint`, `cargo test --workspace`, `just coverage` (≥ 80 % lines);
  - sidecar: `just test-dpi` and `cargo clippy --manifest-path backend/dpi/Cargo.toml --all-targets -- -D warnings` (≥ 80 % coverage excluding `ndpi-sys`);
  - frontend: `pnpm lint`, `pnpm type-check`, `pnpm test:unit`, `pnpm test:e2e`;
  - fuzzing: `just fuzz-dpi 600` (SC-008).
- [X] T083 [P] Update the documentation:
  - `docs/routeros.md`: a "Reply direction (for application classification)" subsection per contracts/router-capture.md;
  - `README.md`: an optional DPI section and a link to `docs/dpi.md`;
  - the sample config: a commented `[dpi]` block.

  Regenerate or check the frontend API types against `openapi.rs`
- [ ] T084 Run quickstart V1–V10 locally (V2–V4 and V9 need `just dpi-build`), and V11 on the real router with the reply-direction rule over one day. Write `specs/003-ndpi-app-classification/validation.md` in the feature 001/002 format — *partial: see validation.md (nDPI and Docker steps pending)*

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: no dependencies.
- **Foundational (Phase 2)**: depends on Setup. It **blocks** all stories.
- **US1 (Phase 3)**: depends on Foundational. This is the MVP. The sidecar tasks T036–T039 need nDPI; the tmon-side tasks T040–T045 do not, and can proceed in parallel with the fake classifier.
- **US2 (Phase 4)**: depends on Foundational. It needs stored `app` values (T020) and, for the catalog, the supervisor's Hello (T025). It can run in parallel with US1 using the fake classifier.
- **US3 (Phase 5)**: depends on Foundational (T025). It is independent of US1/US2 except for `top_apps` in the status (from Stats).
- **US4 (Phase 6)**: T071 and T072 need only Foundational. T069, T070, T073, T074 and T076 need the US1 sidecar (T036–T039). T077 and T078 can start any time after T003.
- **Polish (Phase 7)**: after all the desired stories.

### Within each story

Tests are written and fail first. Then come core/table modules ([P]), then I/O and supervisor wiring, then storage/API, then UI, then fixtures and fuzzing.

### Key task dependencies

- T016 → T024 → T025 → T026
- T017 → T018 → T019, T020, T022
- T023 → T041 → T042 → T045
- T036 → T037, T038 → T039 → T029
- T040, T041 → T042 → T043 → T045 → T034
- T020 → T051, T052 → T053
- T025 → T065 → T066 → T067 → T068
- T036 → T073, T074 → T069, T070

## Parallel Examples

**Foundational tests** (all at once): T008, T009, T010, T011, T012, T013, T014, T015.

**US1 tests**: T027, T028, T030, T031, T032, T033, T034 (T029 once nDPI is built). **US1 tmon side**: T040, T041, T044 alongside the sidecar track T036–T039.

**US2**: tests T046–T050 at once. Then T055, T056, T058 and T059 in parallel with the backend T051–T054.

**US3**: tests T061–T064 at once. T068 (UI) in parallel with T065–T067 against a mocked `/status`.

**US4**: tests T069–T072 at once. T075, T076, T077 and T078 in parallel.

## Implementation Strategy

### MVP first (US1 only)

1. Phase 1 Setup, then Phase 2 Foundational. Checkpoint: SC-007 holds (features 001 and 002 unchanged, no child process when disabled).
2. Phase 3 US1. **Stop and validate** with quickstart V2–V4 and the real classifier. Then run on the real router with the reply-direction rule (V11 preview).

### Incremental delivery

3. US2 (UI): makes the applications visible and usable in rules.
4. US3 (health): makes classifier failures visible. Ship it before announcing the feature.
5. US4 (sandbox, memory, fuzz, packaging): **required before enabling on an untrusted network** or publishing the `-dpi` image.
6. Polish, then validation.md.

### Notes

- Never edit a shipped migration. Migration 7 is new.
- No payload is persisted or logged by either process (FR-024). Only app, category, confidence and host leave the sidecar.
- The main tmon binary must never link `libndpi`. Check with `ldd target/release/tmon | grep -c ndpi` = 0 in T082.
- The reply-direction orientation fix (T023) changes the feature 002 processor. Re-run the 002 suites, the fuzz regressions and the capture bench after it.
- On WSL2 mirrored mode, Landlock/seccomp tests may report "unavailable". The sandbox validation (V9) needs native Linux or Docker.
