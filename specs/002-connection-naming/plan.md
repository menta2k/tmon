# Implementation Plan: Connection Naming from Router Packet Capture

**Branch**: `002-connection-naming` | **Date**: 2026-11-01 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/002-connection-naming/spec.md`

## Summary

Add an optional second input to tmon:
- **What arrives**: the router streams the first ~6 KB of every LAN connection, plus DNS answers, as TZSP over UDP.
- **What a new crate extracts**: a new crate `tmon-capture` decodes the frames and pulls out:
  - TLS ClientHello SNI/ALPN (with TCP reassembly for post-quantum-sized hellos);
  - QUIC v1/v2 Initial SNI/ALPN (RFC 9001/9369 Initial decryption, with CRYPTO reassembly across reordered packets);
  - DNS A/AAAA/CNAME answers bound to the requesting device;
  - the plain-HTTP Host header.
- **Where the results go**: a bounded, sharded connection table keyed by the LAN-side 5-tuple. During enrichment, the existing pipeline looks names up through a new `ConnectionNamer` trait, so existing hostname rules match with "detected" confidence. DNS-learned names also feed `remote_hostname` and the 15-minute back-fill.
- **What else is stored**: the name's source is persisted per flow and shown in the UI. Capture health (silent, FastTrack bypass, WSL large-datagram loss) is reported on the Status page.
- **Libraries**: all parsing uses existing, fuzzed crates (`etherparse`, `tls-parser`, `hickory-proto`, `httparse`, `ring`), plus our own cargo-fuzz targets.

## Technical Context

**Language/Version**: Rust 1.9x stable (2024 edition, as in feature 001); TypeScript 5 / Vue 3 for the UI changes

**Primary Dependencies**:
- new: `etherparse` 0.21, `tls-parser` 0.12, `hickory-proto` 0.26 (default-features off);
- already in tree: `ring` 0.17 (HKDF, AES-128-GCM, QUIC header protection) and `httparse` 1.10;
- details in [research.md](research.md) R3–R7.

**Storage**: DuckDB/Parquet (existing). Migration 6 adds a nullable `remote_host_source UTINYINT` column to raw flows. Old Parquet files are read with `union_by_name` (R11). The connection table and device bindings live in memory only (FR-016).

**Testing**:
- `cargo test`: unit tests, including the RFC 9001/9369 test vectors, and integration tests that replay generated TZSP pcaps;
- `cargo fuzz`: 4 new targets;
- Criterion `capture_pipeline` bench;
- Vitest for the UI;
- Playwright: Status capture card.

**Target Platform**: Linux (native, Docker). WSL2 mirrored networking is detected and reported as unsuitable (R10).

**Project Type**: Web application (existing `backend/` Rust workspace + `frontend/` Vue SPA)

**Performance Goals**: ≥ 50k capture datagrams/s on one core (bench). Home load is ≤ 200 new connections/s. Flow ingest throughput unchanged (SC-004, re-run of `scripts/bench-ingest.sh` with capture on).

**Constraints**:
- < 64 MB extra memory at defaults (SC-004);
- capture never blocks flow ingest: bounded channel, drop and count (FR-015);
- no payload persisted (FR-016);
- off by default (FR-001, SC-007).

**Scale/Scope**: One router. Up to 100k tracked connections and 200k device bindings (configurable). Six user-visible changes:
- `[capture]` config;
- the Status capture card and warnings;
- the name-source tag in the Flows view and host drill-down;
- `/flows` and CSV fields;
- `/metrics` counters;
- the router setup guide.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

`.specify/memory/constitution.md` is still the unfilled template. As in feature 001, the user's global engineering rules are applied as provisional gates:

| Gate | Pre-research | Post-design | Evidence |
|---|---|---|---|
| TDD, ≥ 80 % coverage, unit + integration + e2e | ✅ planned | ✅ | RFC test vectors first; pcap-replay integration tests; Playwright Status card; coverage gate unchanged |
| Immutability | ✅ | ✅ | Extracted `ConnName` values are immutable `Arc`s. The table only inserts or replaces entries, never mutates a published name. `FlowRecord` gains fields via `with_*` builders |
| Small focused files (≤ 400 typical, 800 max) | ✅ | ✅ | One module per concern in `tmon-capture` (layout below) |
| Input validation at boundaries | ✅ | ✅ | All capture input is untrusted: source allow-list, bounds-checked parsers, per-connection and global caps, fuzzing (R13) |
| Error handling, no silent swallowing | ✅ | ✅ | Every reject, unsupported, parse error and eviction is counted (Status + `/metrics`) |
| Security: secrets, auth, rate limiting | ✅ | ✅ | No new secrets. Capture port is allow-listed and bounded. New API fields sit behind existing auth. Payloads never stored |
| Research & reuse before writing | ✅ | ✅ | Existing crates for every protocol (R3–R7). Only the QUIC key schedule (~100 lines on `ring`) and TZSP framing (~50 lines) are hand-written, both pinned by spec test vectors |
| Simplicity | ✅ | ✅ | Same process. In-memory table. One new crate for the hostile parsing surface. No new services |

**Result: PASS.** No violations, so Complexity Tracking is empty.

## Project Structure

### Documentation (this feature)

```text
specs/002-connection-naming/
├── spec.md
├── plan.md                    # this file
├── research.md                # R1–R14 decisions
├── data-model.md              # in-memory entities, flow column, counters
├── quickstart.md              # validation scenarios V1–V10
├── contracts/
│   ├── capture-input.md       # TZSP datagram contract + [capture] config
│   ├── router-capture.md      # RouterOS rules (capture, DNS, FastTrack)
│   └── http-api.md            # /status, /flows, /metrics deltas
├── checklists/requirements.md
└── tasks.md                   # /speckit-tasks output
```

### Source Code (repository root)

```text
backend/
├── crates/
│   ├── tmon-core/src/
│   │   ├── naming.rs                  # NEW: ConnKey, ConnName, NameSource, trait ConnectionNamer
│   │   ├── flow.rs                    # + remote_aliases, remote_host_source (with_* builders)
│   │   └── rules/matcher.rs           # hostname rules also match aliases
│   ├── tmon-capture/                  # NEW crate (hostile-input surface)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── tzsp.rs                # R1 framing
│   │   │   ├── frame.rs               # R3 etherparse → Packet {key, dir, tcp seq/flags, payload}
│   │   │   ├── tcp_reasm.rs           # R4 bounded in-order reassembly
│   │   │   ├── tls.rs                 # R4 ClientHello → SNI, ALPN, ECH flag
│   │   │   ├── quic/{mod,keys,header,crypto_frames}.rs   # R5
│   │   │   ├── dns.rs                 # R6 answers → bindings
│   │   │   ├── http.rs                # R7 Host
│   │   │   ├── table.rs               # R12 sharded ConnTable + DeviceBindings (implements ConnectionNamer)
│   │   │   ├── processor.rs           # per-datagram dispatch, owns reassembly state
│   │   │   ├── health.rs              # R10 counters + detectors
│   │   │   ├── listener.rs            # UDP socket → bounded channel → processor
│   │   │   └── fixtures/              # etherparse-built TZSP packets (feature "fixtures")
│   │   ├── tests/                     # rfc9001_vectors, tls_split, quic_reorder, dns_cname, health, table_limits
│   │   ├── benches/capture_pipeline.rs
│   │   └── examples/gen_capture_fixtures.rs
│   ├── tmon-enrich/src/pipeline.rs    # optional Arc<dyn ConnectionNamer>; lookup order FR-010
│   ├── tmon-store/src/duck/
│   │   ├── schema.rs                  # migration 6: remote_host_source
│   │   ├── files.rs, recategorize.rs  # union_by_name for old Parquet
│   │   ├── ingest.rs, rows.rs         # new column
│   │   └── query/flows.rs             # remote_host_source in rows + CSV
│   ├── tmon-api/src/
│   │   ├── routes/status.rs           # capture section
│   │   ├── warnings.rs                # capture_silent / _large_packets_lost / _fastpath_suspected
│   │   └── metrics.rs                 # tmon_capture_* counters
│   └── tmon/src/{config.rs, app.rs}   # [capture] section; wiring; DNS names → remote_hostname + back-fill
├── fuzz/fuzz_targets/{capture_datagram,tls_client_hello,quic_initial,dns_response}.rs
└── tests/
    ├── fixtures/capture/*.pcap        # generated TZSP captures
    └── us7_capture.rs                 # end-to-end: capture + flow replay → named, categorized flows

frontend/src/
├── api/types.ts                       # CaptureStatus, FlowRecord.remote_host_source
├── components/HostSourceTag.vue       # NEW: tls/quic/http/dns/router tag
├── views/StatusView.vue               # capture card
├── components/CaptureStatusCard.vue  # NEW: Status capture card
└── views/FlowsView.vue, HostDetailView.vue   # source tag next to remote host
frontend/tests/{unit/capture.spec.ts, unit/hostSource.spec.ts, e2e/status.spec.ts}

docs/routeros.md                       # capture section (rules, FastTrack, verify, WSL note)
```

**Structure Decision**: Keep the existing web-application layout, add one backend crate (`tmon-capture`), and touch existing crates only at the seams: the trait in `tmon-core`, the lookup in `tmon-enrich`, the column in `tmon-store`, plus status/metrics and config wiring.

## Architecture & Data Flow

```text
Router ──TZSP/UDP:37008──► capture listener ──bounded chan (drop+count)──► CaptureProcessor (1 task)
                                                                            │ tzsp → frame → by proto:
                                                                            │   TCP → tcp_reasm → tls / http
                                                                            │   UDP/443 → quic (decrypt Initial → CRYPTO reasm → tls)
                                                                            │   UDP/53 resp → dns → DeviceBindings
                                                                            ▼
                                                        ConnTable (16 shards, LRU, TTL)  ◄── health counters
Router ──IPFIX──► collector ──► Pipeline::enrich(raw)
                                  1. LocalNetworks::to_record (un-NAT, feature 001)
                                  2. geo
                                  3. name: ConnTable.lookup(key) → DeviceBindings(lan, remote) → router DNS cache
                                  4. rules.categorize (hostname + aliases) → confidence "detected"
                                  ▼
                               storage (remote_host, remote_host_source)
DNS bindings ──(every 5 s, new names only)──► remote_hostname upsert ──► back-fill (feature 001)
```

## Delivery Phases (input for /speckit-tasks)

1. **Setup**: the crate, dependencies, fuzz scaffolding, fixture generator skeleton.
2. **Foundational** (blocks all stories):
   - `tmon-core` naming types and trait;
   - `FlowRecord` fields;
   - alias-aware matcher;
   - TZSP + frame decode;
   - `ConnTable` with limits;
   - listener + processor skeleton;
   - `[capture]` config;
   - pipeline lookup;
   - migration 6 + `union_by_name`.
3. **US1 (P1)**: TLS (split hellos, ECH), QUIC v1/v2 (vectors, reordered CRYPTO), HTTP Host, end-to-end naming, source tag in the UI. **MVP.**
4. **US2 (P2)**: DNS answers → device bindings with aliases, `remote_hostname` feed + back-fill.
5. **US3 (P2)**: health counters, the three warnings, the Status card, `/metrics`, router guide.
6. **US4 (P3)**: allow-list, flood/limit tests, back-pressure, fuzz runs, the disabled-mode regression (SC-007).
7. **Polish**: bench, coverage, quickstart validation on the real router, docs.

## Complexity Tracking

*No violations; nothing to justify.*
