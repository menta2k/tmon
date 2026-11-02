---

description: "Task list for Connection Naming from Router Packet Capture"
---

# Tasks: Connection Naming from Router Packet Capture

**Input**: Design documents from `/specs/002-connection-naming/`

**Prerequisites**: plan.md, spec.md, research.md (R1–R14), data-model.md, contracts/capture-input.md, contracts/router-capture.md, contracts/http-api.md, quickstart.md

**Tests**: INCLUDED. The plan's Constitution Check adopts TDD with ≥ 80 % coverage. In every phase, write the test tasks first and confirm they FAIL before starting the implementation tasks.

**Organization**: Tasks are grouped by user story, in priority order: US1 (P1), US2 (P2), US3 (P2), US4 (P3).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: User story from spec.md (US1–US4)
- Paths are relative to the repository root. `backend/` is the Cargo workspace and `frontend/` the Vue SPA.

---

## Phase 1: Setup

**Purpose**: The new crate, dependencies and scaffolding

- [X] T001 Create crate `backend/crates/tmon-capture` (`Cargo.toml`, empty `src/lib.rs` with module stubs per plan.md) and add it to `members` in `backend/Cargo.toml`. It depends only on `tmon-core`. Add a `fixtures` cargo feature (off by default) gating `src/fixtures/`
- [X] T002 Add the workspace dependencies to `backend/Cargo.toml`: `etherparse = "0.21"`, `tls-parser = "0.12"`, `hickory-proto = { version = "0.26", default-features = false }`. Also reference the already-locked `ring = "0.17"` and `httparse = "1.10"`, then use them from `backend/crates/tmon-capture/Cargo.toml`. Run `cargo tree -d` to confirm no duplicate major versions of `ring`
- [X] T003 [P] Add the fuzz target stubs `capture_datagram`, `tls_client_hello`, `quic_initial` and `dns_response` to `backend/fuzz/Cargo.toml` and `backend/fuzz/fuzz_targets/*.rs` (they compile but are empty, as feature 001 did for netflow/sflow)
- [X] T004 [P] Add the bench stub `backend/crates/tmon-capture/benches/capture_pipeline.rs` (criterion, `harness = false`) and the example skeleton `backend/crates/tmon-capture/examples/gen_capture_fixtures.rs` (`required-features = ["fixtures"]`, writes to `backend/tests/fixtures/capture/`)
- [X] T005 [P] Add the recipes `fixtures-capture` (runs the generator) and `fuzz-capture` (all 4 targets, `-max_total_time` argument) to the `justfile`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The naming seam, flow fields, storage column, config, frame decode, the connection table and the listener/processor skeleton. After this phase, tmon accepts TZSP and a test namer can name flows end-to-end. The extractors come in the stories.

### Tests first

- [X] T006 [P] Unit tests in `backend/crates/tmon-core/tests/naming.rs`:
  - `ConnKey::from_record` maps `local_* → lan_*`.
  - `NameSource` round-trips codes 1–5 and the API strings `tls|quic|http|dns|router`. Unknown codes give `None`.
  - Name normalization: lowercases, strips the trailing `.`, and rejects inputs other than "1–253 bytes, labels 1–63 bytes of `[a-z0-9_-]` (IDNA A-labels allowed)".
  - ALPN validation: "at most 32 bytes, printable ASCII".
- [X] T007 [P] Matcher test in `backend/crates/tmon-core/tests/builtin_rules.rs` (extend): a hostname rule matching only an alias in `remote_aliases` applies, and reports `Confidence::Detected`. `remote_host` is checked before the aliases.
- [X] T008 [P] Migration/storage tests in `backend/crates/tmon-store/tests/migrations.rs` (extend):
  - Migration 6 adds nullable `remote_host_source UTINYINT` to `flows_hot`.
  - `max(version)` is 6. Update the assertion in `backend/crates/tmon-store/tests/writer.rs` to 6.
  - A Parquet file written **without** the column (old schema) is still readable through the `flows` view and through recategorize rewrite/rebuild, with the value NULL.
- [X] T009 [P] Config tests in `backend/crates/tmon/tests/config.rs` (or the existing config test module). Cover the `[capture]` defaults from contracts/capture-input.md, the env override `TMON_CAPTURE__ENABLED`, and validation errors naming the key:
  - bad `listen`;
  - non-IP `allowed_sources`;
  - a limit of 0;
  - `max_reassembly_bytes_per_conn > max_reassembly_bytes_total`;
  - `conn_ttl_secs` outside 60–3600.
- [X] T010 [P] TZSP/frame tests in `backend/crates/tmon-capture/tests/frame.rs`:
  - TZSP v1/type 0/encap 1 with PADDING + TLV tags + END is decoded.
  - Missing END, or a tag past the end → `malformed{tzsp}`.
  - Wrong ver/type/encap → `unsupported{tzsp|encap}`.
  - Ethernet + 0/1/2 VLAN tags; IPv4 and IPv6 with an extension header.
  - IPv4 fragment → `unsupported{fragment}`; non-IP → `unsupported{l3}`.
  - Truncated L4 → `malformed{frame}`.
  - Use `etherparse` builders to build the inputs.
- [X] T011 [P] Table tests in `backend/crates/tmon-capture/tests/table_limits.rs`:
  - Insert-replace of `Named` entries.
  - A lookup refreshes `last_used_ms`, and TTL expiry happens at `last_used_ms + conn_ttl`.
  - LRU eviction at `max_connections`, counted as `conn_evicted{lru}`.
  - Device bindings are capped at `max_device_bindings`, with `bindings_evicted` counted.
  - Exact connection lookup wins over the device binding.
  - Add an `#[ignore] flood` test (1M random keys): `conns_tracked ≤ max_connections`.
- [X] T012 [P] Pipeline test in `backend/crates/tmon-enrich/tests/pipeline_naming.rs`, with a stub `ConnectionNamer`. Check the FR-010 order: exact connection → DNS binding → router `RemoteNames` (setting `remote_host_source = Router`) → ECH outer name as a last resort. Also check that with no namer the output equals feature 001 behaviour.

### Implementation

- [X] T013 Implement `backend/crates/tmon-core/src/naming.rs`: `ConnKey`, `NameSource` (`#[repr(u8)]`, `from_code`, `as_str`), immutable `ConnName { host, aliases (≤ 8), alpn, source, ech_outer }` with a validating constructor, `normalize_host()`, and `trait ConnectionNamer { fn lookup(&self, key: &ConnKey, now_ms: i64) -> Option<Arc<ConnName>>; }`. Export it from `backend/crates/tmon-core/src/lib.rs`
- [X] T014 Add `remote_host_source: Option<NameSource>` and `remote_aliases: Option<Arc<[Arc<str>]>>` to `FlowRecord` in `backend/crates/tmon-core/src/flow.rs`, with the builders `with_remote_name(host, source)` and `with_aliases(..)` (new value, no mutation). Update every `FlowRecord` construction across the crates and tests
- [X] T015 Make `hostname_matches` in `backend/crates/tmon-core/src/rules/matcher.rs` check `remote_host`, then each alias (T007 green)
- [X] T016 Add migration `(6, …)` to `backend/crates/tmon-store/src/duck/schema.rs` (`ALTER TABLE flows_hot ADD COLUMN IF NOT EXISTS remote_host_source UTINYINT`), and never edit a shipped migration. Write the column in `backend/crates/tmon-store/src/duck/ingest.rs` / `rows.rs`. Switch the `read_parquet` calls in `backend/crates/tmon-store/src/duck/files.rs` and `backend/crates/tmon-store/src/duck/recategorize.rs` (rewrite + rebuild) to `union_by_name = true` (T008 green)
- [X] T017 Add the `[capture]` section (`CaptureConfig` plus a validated `CaptureSettings`) to `backend/crates/tmon/src/config.rs`, with the defaults and validation from contracts/capture-input.md. Update `deploy/tmon.example.toml` (or the shipped sample config) with the commented section (T009 green)
- [X] T018 [P] Implement `backend/crates/tmon-capture/src/tzsp.rs` (R1) and `backend/crates/tmon-capture/src/frame.rs` (R3): `decode(&[u8]) -> Result<Packet<'_>, Reject>`, where `Packet { key_parts, l4: Tcp{seq, flags, payload} | Udp{payload} }` and `Reject` maps to the counter kinds (T010 green)
- [X] T019 [P] Implement the counters (`AtomicU64`, the gauges, and the 15 × 1-minute ring for windowed values) and an immutable `CaptureStats` snapshot in `backend/crates/tmon-capture/src/health.rs`. Include every counter listed in data-model.md §4. Detectors come in US3
- [X] T020 Implement `backend/crates/tmon-capture/src/table.rs`:
  - `ConnTable`: 16 shards of `Mutex<lru::LruCache<ConnKey, TrackedConnection>>`.
  - `TrackedConnection { state: Pending | Named(Arc<ConnName>) | Failed(FailReason), first_seen_ms, last_used_ms }`.
  - `DeviceBindings`: an LRU keyed by `(lan_ip, remote_ip)` with `expires_ms`.
  - A `ConnectionNamer` impl (exact connection first, then the binding), plus the `lookups` / `lookup_hits` counters.
  - Lock hold time is a single map operation (T011 green).
- [X] T021 Implement `backend/crates/tmon-capture/src/processor.rs`: `CaptureProcessor::handle(&mut self, src: IpAddr, datagram: &[u8], now_ms)`. It decodes the frame, determines the client direction, counts `tcp_syn_seen` / `tcp_payload_first_seen`, and dispatches to extractor hooks (no-ops for now). Do the same for the listener in `backend/crates/tmon-capture/src/listener.rs`:
  - a UDP socket with `socket_recv_buffer_bytes` and a 65,535-byte read buffer;
  - the source allow-list check (`rejected_source`);
  - a bounded `tokio::mpsc` of `queue_capacity` using `try_send`, where a full channel counts as `dropped_backpressure`;
  - one processor task;
  - a periodic TTL sweep.
- [X] T022 Add an optional `Arc<dyn ConnectionNamer>` to `PipelineDeps` in `backend/crates/tmon-enrich/src/pipeline.rs`, and implement the FR-010 lookup order in `enrich()` (data-model.md §2). Router-cache names set `remote_host_source = Router` (T012 green)
- [X] T023 Wire capture in `backend/crates/tmon/src/app.rs`:
  - When `capture.enabled`, build the `ConnTable`, spawn the listener and processor, and pass the namer into the pipeline.
  - Expose a `CaptureHandle` (stats snapshot, listening/bind error) in `AppState` (`backend/crates/tmon-api/src/state.rs`).
  - A bind failure is logged and recorded, and does not stop the server.
- [X] T024 [P] Add the generic fixture builders to `backend/crates/tmon-capture/src/fixtures/mod.rs`: `tzsp_wrap(eth_frame)`, TCP/UDP frame builders (v4/v6, optional VLAN) built on `etherparse`, and a pcap writer. Reuse the pcap format of `tmon-collector`'s fixtures so `tmon replay` can send them

**Checkpoint**: The workspace builds. The feature 001 suite and benches still pass (SC-007). TZSP is decoded and counted. A stub namer names flows end-to-end.

---

## Phase 3: User Story 1 — Encrypted web and video traffic is identified by name (P1) 🎯 MVP

**Goal**: TLS (including split ClientHellos and ECH), QUIC v1/v2 Initial (with reordered CRYPTO) and plain HTTP names reach flows. Hostname rules apply with "detected" confidence, and the UI shows the name source.

**Independent Test**: quickstart V2, V3 and V4. Replay the capture fixture, then the flow fixture. Flows carry the SNI with `remote_host_source = tls|quic`, the right category, and `by=rule` `confidence = detected`.

### Tests first

- [X] T025 [P] [US1] TLS tests in `backend/crates/tmon-capture/tests/tls.rs`:
  - SNI + first ALPN from a single-record ClientHello.
  - A post-quantum-sized (~1.8 KB) ClientHello split across 2 segments, delivered in order **and** out of order, with a duplicate segment ignored.
  - A gap or overflow beyond `max_reassembly_bytes_per_conn` → `Failed`, `tls_incomplete` counted.
  - The 10 s timeout.
  - The ECH extension `0xfe0d` sets `ech_outer = true`, and `tls_ech` is counted.
  - A malformed record → `tls_parse_error`.
  - Non-TLS payload is ignored.
- [X] T026 [P] [US1] QUIC key schedule tests in `backend/crates/tmon-capture/tests/rfc9001_vectors.rs`:
  - The RFC 9001 Appendix A.1 client `key`/`iv`/`hp`, and the A.2 client Initial decrypted to the expected CRYPTO payload.
  - The RFC 9369 Appendix A.1/A.2 equivalents for v2 (salt `0dede3def700a6db819381be6e269dcbf9bd2ed9`, `quicv2` labels, Initial type `0b01`).
- [X] T027 [P] [US1] QUIC reassembly tests in `backend/crates/tmon-capture/tests/quic_reorder.rs`:
  - CRYPTO split across 2–3 Initials delivered in reverse order is named.
  - Coalesced packets in one datagram are handled.
  - PADDING/PING/ACK frames are skipped.
  - An unknown version → `quic_unsupported_version`.
  - A bad tag → `quic_decrypt_error`.
  - Server → client Initials are not decrypted.
- [X] T028 [P] [US1] HTTP tests in `backend/crates/tmon-capture/tests/http.rs`: `Host` is extracted with the port stripped and lowercased. Non-HTTP payload, and an over-long or invalid host (rejected, `name_invalid`), are both handled.
- [X] T029 [P] [US1] Store/API tests:
  - `backend/crates/tmon-store/tests/flows_query.rs` (or the existing raw-flows test): `remote_host_source` is returned as a string or null.
  - `backend/crates/tmon-api` route test: `/flows` rows and the `/flows/export.csv` header include `remote_host_source` after `remote_host`.
  - `/hosts/{ip}` remote rows include `hostname_source`.
- [X] T030 [P] [US1] End-to-end test in `backend/tests/us7_capture.rs`:
  1. Start the app with `[capture] enabled` on an ephemeral port.
  2. Send the generated TLS-split, QUIC v1-reordered, QUIC v2 and shared-CDN capture fixtures, then the matching IPFIX flows.
  3. Assert the names, sources, categories (from built-in hostname rules) and per-device names on the shared CDN IP (US1 #4), and `GET /traffic/breakdown?by=rule` confidence `detected`.
- [X] T031 [P] [US1] Frontend unit tests in `frontend/tests/unit/hostSource.spec.ts`: `HostSourceTag` renders the label and tooltip for each of `tls|quic|http|dns|router`, and renders nothing for null. The FlowsView spec is extended to show the tag next to the remote host.

### Implementation

- [X] T032 [P] [US1] Implement `backend/crates/tmon-capture/src/tcp_reasm.rs`: per-connection `BTreeMap<u32 offset, Bytes>` relative to the first payload sequence number, with overlap/duplicate trimming, a contiguous-prefix view, the per-connection cap, the global `reassembly_bytes_total` accounting (oldest `Pending` dropped on overflow, `conn_evicted{bytes_cap}`), and the 10 s timeout (R4)
- [X] T033 [P] [US1] Implement `backend/crates/tmon-capture/src/tls.rs`: `client_hello_names(&[u8]) -> Result<Option<HelloInfo>>` using `tls-parser`, with `HelloInfo { sni, alpn_first, ech }`. Expose a shared `from_handshake(&[u8])` for QUIC. The SNI goes through `normalize_host` (T025 green)
- [X] T034 [P] [US1] Implement `backend/crates/tmon-capture/src/quic/keys.rs` (the HKDF-Extract/Expand-Label key schedule on `ring::hkdf`, v1/v2 salts and labels), `quic/header.rs` (long-header parse, header protection removal with `ring::aead::quic::AES_128`, packet-number decode, the coalesced-packet walk) and `quic/crypto_frames.rs` (varint, frame walk, offset-based CRYPTO buffer). Use `ring::aead::AES_128_GCM` in `quic/mod.rs` for the payload decrypt (T026, T027 green)
- [X] T035 [P] [US1] Implement `backend/crates/tmon-capture/src/http.rs` using `httparse`: only a request line starting with a method token, and `Host` normalized (T028 green)
- [X] T036 [US1] Hook TLS, QUIC and HTTP into `backend/crates/tmon-capture/src/processor.rs`: create `Pending` on the first client payload or Initial, publish `Named(Arc<ConnName>)` or `Failed` into the `ConnTable`, and count `*_started`, `*_named` and errors (depends on T032–T035)
- [X] T037 [US1] Expose `remote_host_source` in the raw-flow query and CSV in `backend/crates/tmon-store/src/duck/query/flows.rs`, and in the API row types and CSV writer in `backend/crates/tmon-api/src/routes/flows.rs`. Add `hostname_source` to `/hosts/{ip}` remotes in `backend/crates/tmon-api/src/routes/hosts.rs`. Document it in `backend/crates/tmon-api/src/openapi.rs` (T029 green)
- [X] T038 [US1] Add the US1 fixture generators to `backend/crates/tmon-capture/src/fixtures/` and the example:
  - `tls_split_pq.pcap`;
  - `tls_ech.pcap`;
  - `quic_v1_rfc.pcap` (the RFC 9001 A.2 bytes);
  - `quic_v1_reordered.pcap` (encrypted by the generator with the `keys.rs` schedule);
  - `quic_v2_rfc.pcap`;
  - `http_host.pcap`;
  - `shared_cdn.pcap`.

  Also generate the matching IPFIX flow pcap `capture_flows.pcap` using the `tmon-collector` fixture encoder. Commit the generated files under `backend/tests/fixtures/capture/` (T030 green)
- [X] T039 [P] [US1] Frontend:
  - Add `NameSource` and `FlowRow.remote_host_source` to `frontend/src/api/types.ts`, and the labels/tooltips to `frontend/src/constants.ts`.
  - Create `frontend/src/components/HostSourceTag.vue`.
  - Render it next to the remote host in `frontend/src/views/FlowsView.vue` and `frontend/src/views/HostDetailView.vue` (T031 green).
- [X] T040 [US1] Fill in `capture_datagram`, `tls_client_hello` and `quic_initial` in `backend/fuzz/fuzz_targets/`. The first runs through one stateful `CaptureProcessor` so that reassembly state is exercised. Seed the corpora from the US1 fixtures. Run each for 5 minutes and add any crashers as regression tests

**Checkpoint**: US1 is fully functional. quickstart V2–V4 pass. This is the MVP.

---

## Phase 4: User Story 2 — Names from DNS answers, per device (P2)

**Goal**: DNS answers produce per-device bindings with alias chains. Bindings name flows that have no setup name, feed `remote_hostname` and trigger the back-fill.

**Independent Test**: quickstart V5. Replay `dns_cname.pcap`, then a non-443 flow. The flow is named with the queried name and `source = dns`, and a rule matching only the CNAME target applies. `remote_hostname` is updated within 5 s, and an earlier flow is renamed by the back-fill.

### Tests first

- [X] T041 [P] [US2] DNS tests in `backend/crates/tmon-capture/tests/dns_cname.rs`:
  - A NOERROR response with qname → CNAME → CNAME → A/AAAA gives bindings `(response dst, each address)`, with `host` = the qname, the aliases as the chain in order (at most 8), and `source = Dns`.
  - `expires_ms = now + clamp(ttl, 10 min, 1 h)`.
  - NXDOMAIN, queries (QR = 0) and malformed messages are ignored or counted as `dns_parse_error`.
  - Two devices resolving the same IP to different names keep separate bindings (US2 #2).
  - A newer answer replaces the older binding.
- [X] T042 [P] [US2] Feed test in `backend/crates/tmon-capture/tests/pending_names.rs`: `PendingRemoteNames` collapses duplicates, caps at "at most 10k per drain", and drain returns an immutable batch.
- [X] T043 [P] [US2] Integration test in `backend/tests/us7_capture.rs` (extend):
  - Replay the DNS CNAME fixture and a flow to the resolved IP on a non-web port. The flow is named via DNS, and a test rule matching only the CNAME target categorizes it.
  - A flow stored **before** the DNS answer is renamed after the 5 s drain plus back-fill, and its `remote_host_source = 4`.

### Implementation

- [X] T044 [US2] Implement `backend/crates/tmon-capture/src/dns.rs` using `hickory-proto` message parsing (UDP only, QR = 1, NOERROR). Build the CNAME chain, then the address set, and normalize every name. Return immutable `Binding` values (T041 green)
- [X] T045 [US2] Hook DNS into `backend/crates/tmon-capture/src/processor.rs` for UDP src port 53. Insert into `DeviceBindings` and push `(remote_ip, host)` into `PendingRemoteNames` in `backend/crates/tmon-capture/src/pending.rs`. Count `dns_responses` and `dns_bindings` (T042 green)
- [X] T046 [US2] In `backend/crates/tmon/src/app.rs`, add a 5 s task that drains `PendingRemoteNames`, upserts the names into the store's `remote_hostname` (the path `apply_dns` in `backend/crates/tmon-enrich/src/router_poll.rs` uses), merges them into the pipeline's `RemoteNames` via `set_remote_names` (as a new map), and calls `backfill_hostnames`. Extend `backend/crates/tmon-store/src/duck/backfill.rs` so back-filled rows get `remote_host_source` (4 for capture DNS, 5 for router poll) (T043 green)
- [X] T047 [US2] Fill in the `dns_response` fuzz target in `backend/fuzz/fuzz_targets/dns_response.rs`. Add the `dns_cname.pcap` generator to `backend/crates/tmon-capture/src/fixtures/` and commit the output. Run the target for 5 minutes

**Checkpoint**: US1 and US2 both work independently. quickstart V5 passes.

---

## Phase 5: User Story 3 — Know whether capture is working, and how to set it up (P2)

**Goal**: The Status page shows capture health and counters. The three warnings fire with remediation text. `/metrics` exposes the counters, and the router guide is complete.

**Independent Test**: quickstart V6 and V7. Enable capture with nothing sent → `capture_silent`. Replay `wsl_truncated.pcap` → `capture_large_packets_lost`. Replay `fasttrack_syn_only.pcap` → `capture_fastpath_suspected`. The Status card shows the counters moving.

### Tests first

- [X] T048 [P] [US3] Detector unit tests in `backend/crates/tmon-capture/tests/health.rs`, at the exact thresholds from research R10:
  - silent at 300 s (and when nothing has ever arrived);
  - large-packet loss at `tls_started ≥ 50 && tls_incomplete/tls_started ≥ 0.5 && max_datagram_len ≤ 1472`, and not at 49 starts or at max length 1,473;
  - fast-path at `tcp_syn_seen ≥ 100 && tcp_payload_first_seen/tcp_syn_seen < 0.1`;
  - the 15-minute window rolls off old buckets.
- [X] T049 [P] [US3] API tests in `backend/crates/tmon-api/tests/status_capture.rs`:
  - `/status.capture` has the shape from contracts/http-api.md when enabled, and is `{"enabled": false}` when disabled.
  - `warnings[]` contains `capture_silent`, `capture_large_packets_lost` and `capture_fastpath_suspected` when the corresponding stats are injected.
  - A bind failure gives `collector_down:37008`.
  - `/metrics` contains every `tmon_capture_*` series listed in the contract.
  - `named_share_15m` is null when there were no lookups.
- [X] T050 [P] [US3] Frontend tests:
  - `frontend/tests/unit/capture.spec.ts`: the Status capture card renders the counters, names by source and the named share; shows a disabled state; and the warning banner links to the setup guide.
  - `frontend/tests/e2e/status.spec.ts` (extend): with the mocked API, the capture card is visible.

### Implementation

- [X] T051 [US3] Implement the detectors in `backend/crates/tmon-capture/src/health.rs` (`const` thresholds, `fn warnings(&CaptureStats, now_ms) -> Vec<CaptureWarning>`) and the windowed `max_datagram_len`, TLS start/incomplete and SYN/payload buckets (T048 green)
- [X] T052 [US3] Add `backend/crates/tmon-api/src/routes/status.rs` (the `capture` object built from `CaptureHandle`). In `backend/crates/tmon-api/src/warnings.rs`, map the capture warnings to `Warning::new(code, message)` with the remediation text from contracts/http-api.md, and the bind error to `collector_down:<port>`. The existing SSE `warnings` push picks them up. Document the schema in `backend/crates/tmon-api/src/openapi.rs`
- [X] T053 [US3] Export the `tmon_capture_*` series in `backend/crates/tmon-api/src/metrics.rs`, with the names and labels from contracts/http-api.md (T049 green)
- [X] T054 [P] [US3] Frontend:
  - Add the `CaptureStatus` type to `frontend/src/api/types.ts`.
  - Create `frontend/src/components/CaptureStatusCard.vue`: listening state, datagrams, names by source, errors, evictions and named share, using the existing `StatTile.vue`.
  - Add it to `frontend/src/views/StatusView.vue`.
  - Map the warning codes to setup-guide links in `frontend/src/constants.ts` (T050 green).
- [X] T055 [US3] Generate `wsl_truncated.pcap` (≥ 60 ClientHellos with every datagram ≤ 1,472 bytes, so the second segments are missing) and `fasttrack_syn_only.pcap` (≥ 120 SYNs, no payload) in `backend/crates/tmon-capture/src/fixtures/`, and commit them. Extend `backend/tests/us7_capture.rs` so that replaying each one raises the matching warning
- [X] T056 [P] [US3] Write the capture section of `docs/routeros.md` from contracts/router-capture.md: the rules for IPv4 and IPv6, the loop-avoidance note, the FastTrack `connection-bytes=6000-0` change, the bandwidth cost, the verification steps, the WSL2 limitation with its alternatives, and the inbound-connection limitation. Link it from `README.md`

**Checkpoint**: US3 works. quickstart V6 and V7 pass.

---

## Phase 6: User Story 4 — Capture stays safe, bounded and optional (P3)

**Goal**: The allow-list, bounded memory under flood, back-pressure that never touches flow ingest, no payload kept, and unchanged behaviour when disabled.

**Independent Test**: quickstart V1 and V10.

### Tests first

- [X] T057 [P] [US4] Allow-list tests in `backend/crates/tmon-capture/tests/listener.rs`:
  - A datagram from a non-allowed IP is dropped and `rejected_source` counted.
  - `allowed_sources` empty → falls back to `collector.allowed_exporters`.
  - Both empty → all senders accepted.
- [X] T058 [P] [US4] Back-pressure test in `backend/crates/tmon-capture/tests/listener.rs`: with a stalled processor and `queue_capacity = 4`, extra datagrams count as `dropped_backpressure`, and the socket task never blocks.
- [X] T059 [P] [US4] Isolation test in `backend/tests/us7_capture.rs` (extend): blast 200k random datagrams at the capture port while replaying the feature 001 flow fixture. All flow records are stored (0 flow drops), the capture counters show malformed/unsupported, and `conns_tracked ≤ max_connections`.
- [X] T060 [P] [US4] Payload-retention test in `backend/crates/tmon-capture/tests/table_limits.rs` (extend): after a TLS/QUIC/HTTP connection is named, `TrackedConnection` holds no payload bytes (`Named`/`Failed` sizes are fixed), and `reassembly_bytes` returns to 0.
- [X] T061 [P] [US4] Disabled-mode regression in `backend/tests/us1_dashboard.rs` (extend), or a new test: with no `[capture]` section, no socket is bound on 37008, `/status.capture == {"enabled": false}`, and stored rows have `remote_host_source` NULL or `router` only.

### Implementation

- [X] T062 [US4] Finish the allow-list fallback and the startup log line ("capture accepting from all senders") in `backend/crates/tmon-capture/src/listener.rs` and `backend/crates/tmon/src/app.rs` (T057, T058 green)
- [X] T063 [US4] Harden the processor in `backend/crates/tmon-capture/src/processor.rs`:
  - Duplicate captures are idempotent: the same segment or Initial seen twice is not double-counted as started.
  - `Failed` entries suppress restarts.
  - Every parse path returns `Result` and is counted, never panics.
  - Remove any `unwrap`/indexing on input data.

  Fix whatever T059 and T060 expose
- [X] T064 [US4] Run all 4 capture fuzz targets for 10 minutes each (`just fuzz-capture 600`), add crashers as regression tests in `backend/crates/tmon-capture/tests/fuzz_regressions.rs`, and record the results for validation

**Checkpoint**: All user stories work. quickstart V1 and V10 pass.

---

## Phase 7: Polish & Cross-Cutting Concerns

- [X] T065 [P] Fill in the bench `backend/crates/tmon-capture/benches/capture_pipeline.rs` (it replays the fixture mix through `CaptureProcessor`) and confirm ≥ 50k datagrams/s on one core. Run `scripts/bench-ingest.sh` with capture enabled and idle, and confirm ingest stays within 5 % of the feature 001 baseline (SC-004)
- [X] T066 [P] Measure memory: RSS with capture on, at defaults, under the `flood` test. It must be < 64 MB above the capture-off baseline (SC-004, SC-005). Record it in `specs/002-connection-naming/validation.md`
- [X] T067 [P] Code review of `tmon-capture` (code-reviewer + security-reviewer agents). Address CRITICAL/HIGH findings. Check that files stay ≤ 400 lines and functions < 50 lines
- [X] T068 Run the gate: `cargo fmt --check`, `cargo lint`, `cargo test --workspace`, `just coverage` (≥ 80 % lines backend + frontend), `pnpm lint`, `pnpm type-check`, `pnpm test:unit` and `pnpm test:e2e`
- [X] T069 Update `README.md` (feature overview, `[capture]` config, link to the router guide) and the sample config. Make sure `openapi.rs` and the generated `frontend/src/api` types are regenerated and match
- [ ] T070 Run quickstart V1–V7 and V10 locally, and V8–V9 on the real router (tmon running natively or in Docker). Write `specs/002-connection-naming/validation.md` in the feature 001 format

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: no dependencies.
- **Foundational (Phase 2)**: depends on Setup. It **blocks** all stories.
- **US1 (Phase 3)**: depends on Foundational. This is the MVP.
- **US2 (Phase 4)**: depends on Foundational only. It can run in parallel with US1; the shared `processor.rs` hook-ups T036 and T045 touch the same file, so do them one after the other.
- **US3 (Phase 5)**: depends on Foundational. Its detectors use the TLS counters, so T055 needs T036 from US1 for realistic fixtures. T048–T054 can start as soon as Foundational is done.
- **US4 (Phase 6)**: depends on Foundational. T059 and T063 are most useful after US1 and US2 extractors exist.
- **Polish (Phase 7)**: after all the desired stories.

### Within each story

Tests are written and fail first. Then come the extractors/modules ([P]), then the processor hook-up, then API/UI, then fixtures and fuzzing.

### Key task dependencies

- T013 → T014 → T015, T022
- T016 depends on T014. T018 → T021. T019, T020 → T021 → T023. T022 → T023
- T032, T033, T034, T035 → T036 → T038
- T044 → T045 → T046
- T051 → T052, T053

## Parallel Examples

**Foundational tests** (all at once): T006, T007, T008, T009, T010, T011, T012.

**US1 tests**: T025, T026, T027, T028, T029, T030, T031. **US1 modules**: T032, T033, T034, T035, T039 (different files).

**US3**: T048, T049, T050 in parallel. Then T054 and T056 in parallel with T051–T053.

**US4 tests**: T057–T061 in parallel.

## Implementation Strategy

### MVP first (US1 only)

1. Phase 1 Setup, then Phase 2 Foundational. Checkpoint: SC-007 holds (feature 001 unchanged).
2. Phase 3 US1 (TLS + QUIC + HTTP). **Stop and validate** with quickstart V2–V4. Then try on the real router with tmon running natively or in Docker (V8 subset), because this is where most of the "guessed" traffic goes away.

### Incremental delivery

3. US2 (DNS bindings): names non-web traffic and makes naming faster.
4. US3 (health + guide): makes misconfiguration visible. Ship it before telling other users about the feature.
5. US4 (hardening): flood/isolation tests and long fuzz runs. **Required before running against an untrusted network**.
6. Polish, then validation.md.

### Notes

- Never edit a shipped migration. Migration 6 is new.
- No packet payload is ever persisted (FR-016). Only names, ALPN and the source are kept.
- On WSL2 mirrored mode, only the fixture-replay scenarios work (datagrams ≤ 1,472 bytes). The real-router validation needs native Linux or Docker.
