# Validation results: quickstart V1–V11 (feature 003)

**Date**: 2026-11-01
**Build**: debug for the test suites, release for the benches, on WSL2 (12 vCPU, 30 GB RAM)
**Classifier**: **nDPI is not built on this machine.** `just dpi-build` (nDPI 6.0, `./configure && make`) has not been run yet. Every scenario that needs the real sidecar is marked ⏸. The tmon side was validated with the pure-Rust `fake_dpi` classifier, which speaks the same protocol v1 (`backend/crates/tmon-dpi-client/examples/fake_dpi.rs`). The sidecar's pure-Rust core (result mapping, flow table, I/O loop, allocator accounting) was tested with a mock engine (`--no-default-features`). The nDPI-facing code (`ndpi.rs`, `sandbox.rs`, the `ndpi-sys` bindings) passes typecheck and clippy against the nDPI 6.0 headers.
**Fixtures**: synthetic, deterministic (`tmon-capture/examples/gen_app_fixtures.rs`), in `backend/tests/fixtures/capture/apps_*.pcap`.

| # | Scenario | Result | Evidence |
|---|---|---|---|
| V1 | Disabled = unchanged | ✅ PASS | `us8_dpi::disabled_dpi_spawns_nothing_and_changes_nothing`: no child process (checked via `/proc`), `/status.dpi == {"enabled": false}`, no `dpi_*` warnings. All feature 001 and 002 suites green |
| V2 | Apps without names | ✅ tmon side; real nDPI ✅ in the Docker smoke test (6/6 confident) | `us8_dpi::fake_classifier_names_apps_end_to_end`: the app mix replayed through capture → classifier → enrichment → store. Every fixture connection gets the expected app and tmon category, `by=app` lists them, and the late `port_only` answer is back-filled. Real-nDPI variant: `us8_dpi::real_ndpi_…` (ignored; `TMON_DPI_BIN`), `tmon-dpi/tests/apps.rs` |
| V3 | Precedence | ✅ PASS (fake) | Same e2e test: `zoom_tls` keeps `remote_host = zoom.us` (source `tls`) and gets `app = Zoom`. The hostname rule (900k) beats the app rule (860k), and a user rule beats both (`tmon-core/tests/builtin_app_rules.rs`, `rules_api.rs`) |
| V4 | Guess vs confident | ✅ PASS (fake) | `port_only` → `app_confident = false`, rule confidence `inferred` (e2e + `apps_api.rs`) |
| V5 | UI | ✅ PASS | Vitest `app.spec.ts`, `appBreakdown.spec.ts`, `appRules.spec.ts`, `dpi.spec.ts`. Playwright (mocked API): app filter, CSV header ends with `app,app_confident`, Status DPI card |
| V6 | Crash and restart | ✅ PASS (fake) | `tmon-dpi-client/tests/supervisor.rs` and `health.rs`: crash → restart with backoff → `running`. 5 failures in 10 min → `failed` + `dpi_failed`, flows unaffected |
| V7 | Version and missing binary | ✅ PASS | (a) `us8_dpi::missing_binary_warns_and_tmon_keeps_serving` → `dpi_not_found`; (b) `handshake.rs`: `--proto-version 2` → `incompatible`, no restart loop |
| V8 | Overload | ✅ PASS (unit/integration) / ⚠ RSS | `backpressure.rs::slow_classifier_drops_at_the_queue_without_blocking`: `dropped_queue` > 0, the capture path never blocks; `health.rs::drop_share_over_the_window_warns`: `dpi_dropping` above 5 %. See *Memory* for the RSS bound |
| V9 | Sandbox and memory | ⏸ PENDING (real nDPI) | `tmon-dpi/tests/sandbox.rs` (SIGSYS on `socket`/`open`, `NoNewPrivs: 1`, `Seccomp: 2`) and `memory.rs` are written and typechecked; they run after `just dpi-build` |
| V10 | IPC fuzz | ✅ PASS (codec) / ⏸ `sidecar_packets` | `dpi_proto_to_sidecar` and `dpi_proto_to_tmon`, 30 s each with fixture-derived seeds: 0 crashes. `sidecar_packets` (real nDPI, ASan) needs `just dpi-build`. A 600 s run is still to do |
| V11 | Real network (SC-001) | ⏸ PENDING | Needs the router with the reply-direction rule (`docs/routeros.md`) and one day of traffic |

## Docker image smoke test (T077), real nDPI

- **Image:** `just docker-dpi` builds `tmon:dpi` and the base `tmon`. A check confirms the base image contains no `libndpi`.
- **Run:** `tmon:dpi` with `deploy/tmon.dpi.toml`, using `--read-only --cap-drop ALL --security-opt no-new-privileges`.
- **Startup:** the sidecar reaches `running` with engine `nDPI 6.0.0-1-1a52933 (API 15)` and 485 applications. The sandbox log shows `no_new_privs`, `landlock`, `rlimit_as` and `seccomp` all applied (`drop_root: not root`, since the image runs as nonroot).
- **Classification:** `apps_mix.pcap` was replayed to the capture port. All 6 apps were classified **confident** under seccomp: BitTorrent, OpenVPN, RDP, STUN, Source_Engine, WireGuard. 0 restarts, 0 invalid packets, nDPI memory 9.7 MB.
- **Fixed by this test:**
  - The nDPI library install did not install `libndpi.pc`, so the build could not find nDPI. Fixed with `make install-pkgconfigDATA`.
  - A `getppid() == 1` check added during review killed the sidecar in containers, where tmon is PID 1. Removed; a dead tmon is detected by socket EOF.
  - Sidecar stderr is now forwarded at info level.

## Real router traffic, first run (Docker Desktop on Windows, WSL2 mirrored)

- **Large datagrams:** full 1,519-byte TZSP datagrams reach the container through Docker Desktop's port forwarder intact.
- **Port 37008 can't be published:** Docker Desktop cannot publish host UDP 37008, because WSL2 mirrored mode reserves the Linux ephemeral range 33700–37795 on Windows. The DPI compose file now uses host port `38008:37008/udp`.
- **Sender filters must use the gateway:** Docker Desktop shows every sender as the compose network gateway (`172.20.0.1`). `allowed_exporters`/`allowed_sources` must name the gateway, and the real filter on the router's address is the Windows firewall rule.
- **Sidecar crash on real traffic:** the sidecar was killed with SIGSYS on `openat` (syscall 257) by real traffic, which the fixtures never triggered. Seccomp now answers `open`/`openat`/`openat2` with `EACCES` instead of killing. Two stacked filters are used, and the strictest verdict wins. Landlock still denies file access.

## Benches and memory (T079, T080)

| Measurement | Result |
|---|---|
| Criterion `tmon-dpi-client/benches/tap_pipeline` (002 scenarios + app mix × 40) | without tap **9.1 M dgrams/s**, with a running `DpiTap` **3.7 M dgrams/s** on one core (target ≥ 50 k) ✅ |
| `tmon-dpi/benches/classify` (real nDPI, ≥ 20 k packets/s) | ⏸ written, needs `just dpi-build` |
| `CAPTURE=1 CAPTURE_RATE=5000 bench-ingest.sh 10000 60` (baseline) | 584,700 flows (9,745/s), 0 drops, 300k capture datagrams, peak RSS **267 MB** |
| `DPI=1 CAPTURE_RATE=5000 bench-ingest.sh 10000 60` (`fake_dpi`) | 588,400 flows (9,807/s), 0 drops, 300k packets sent to the classifier, 0 `dropped_queue`, 0 restarts. Ingest is within 5 % of the baseline (SC-003) ✅. Peak RSS **333 MB (+66 MB)**, over the **+16 MB** target ⚠. Sidecar (fake) peak 17 MB |

**RSS finding.** The +66 MB comes from a single pair of 60 s runs. VmHWM varied by about 50 MB between identical runs in feature 002's validation (210 vs 257 MB), so this needs repeated runs before drawing a conclusion. The expected DPI share is:
- the app table: 2 × 50k × ~120 B ≈ 12 MB;
- the queue: ≤ 16 MB `queue_max_bytes`;
- the per-connection budgets: about 4 MB.

That adds up to roughly 32 MB at worst, so the +16 MB target in tasks.md is itself tighter than the configured queue bound. **Open:** repeat 3× each, and either lower the `queue_max_bytes` default or revise the target.

## Code review (T081)

- **Reviews:** two independent reviews, one for code and one for security. Neither found anything CRITICAL. The fixes below were applied, with regression tests where testable.
  - **HIGH, sidecar socket non-blocking:** the sidecar inherited a non-blocking fd 3 from tmon's socket pair, which caused a busy-spin, and a full socket buffer killed the process. `Seqpacket::from_fd` now clears `O_NONBLOCK`.
  - **HIGH, tap byte counter drift:** the tap's queued-bytes counter could drift upward and then drop every packet. It now reserves before `try_send`, rolls back on failure, and saturates on dequeue (`tests/tap.rs`).
  - **HIGH (security), sandbox fails closed:** every step except Landlock is now required. A failed step exits before serving. `drop_root` verifies that root cannot be regained.
  - **HIGH (security), seccomp allow-list:** `tgkill`, `getpid`, `gettid`, `sendmsg` and `recvmsg` were removed (a sidecar running as tmon's uid could signal tmon). `mmap` and `mprotect` are now allowed only without `PROT_EXEC`.
  - **MEDIUM, fixed:**
    - Packets dropped before the queue no longer use up a connection's packet budget.
    - The tap stops queueing as soon as the session ends, not after the kill grace period.
    - The memory budget excludes the nDPI module's baseline, so a small limit no longer empties the table on every packet.
    - The nDPI flows drop before the module.
    - Classifier stderr is read in bounded 512 B lines.
    - Control messages are rate-limited (50/s; beyond that they count as invalid → restart).
    - Repeated catalog messages are ignored once the catalog is full.
    - App and category names reject markup characters.
    - The nDPI build in Docker is hardened (`-fstack-protector-strong`, `_FORTIFY_SOURCE=2`, full RELRO), with an optional `NDPI_COMMIT` pin.
  - **Deferred, MEDIUM:**
    - `final_set`/`done` entries expire only by LRU, not by time, and `Forget` is never sent.
    - The per-packet name `String`s in `ndpi.rs` are not cached.
    - The `RLIMIT_AS` headroom (+64 MB) has not been measured against real nDPI yet. Check it with `memory.rs`.
- **Rules:**
  - Every file is ≤ 400 lines.
  - `unsafe` is in `ndpi.rs` and `alloc.rs` (nDPI and the allocator), and also in single-syscall blocks in `ipc.rs` (fcntl/setsockopt/recv/send), `sandbox.rs` (prctl/setuid/setrlimit) and `main.rs` (`from_fd`, the self-test `socket`). Every block has a `SAFETY` comment, enforced by `clippy::undocumented_unsafe_blocks`. This is a documented deviation from "only ndpi.rs and alloc.rs".

## Automated suites (T082)

| Suite | Result |
|---|---|
| Backend `cargo test --workspace` | 359 passed, 0 failed, 2 ignored (real-nDPI e2e and an existing one). `cargo fmt --check` and `cargo lint` clean |
| Backend coverage (`just coverage`: `cargo llvm-cov --workspace`) | **89.7 % lines**, 88.7 % functions. `tmon-dpi-client` 56–100 % per file, `tmon-dpi-proto` 98–100 % ✅ |
| Sidecar `cargo llvm-cov -p tmon-dpi --no-default-features` | **91.2 % lines** (the nDPI-only modules are excluded by the feature) ✅ |
| Sidecar clippy `--all-targets -D warnings` | clean both with `ndpi` (headers via `NDPI_INCLUDE_DIR`) and without |
| Frontend lint, type-check | clean |
| Frontend unit (`vitest`) | 229 passed; 96.2 % lines ✅ |
| Playwright | 3 new mocked-API tests passed; the 3 backend-replay tests need a running backend (skipped) |
| `just test-dpi`, `just fuzz-dpi 600` | ⏸ need `just dpi-build` |

## To finish (needs nDPI built)

```bash
just dpi-build                         # nDPI 6.0 → backend/dpi/target/ndpi, then tmon-dpi
just test-dpi                          # T029 apps, T069 sandbox, T070 memory
cargo bench --manifest-path backend/dpi/Cargo.toml -p tmon-dpi --bench classify
TMON_DPI_BIN=backend/dpi/target/release/tmon-dpi cargo test -p tmon --test us8_dpi -- --ignored
DPI=1 TMON_DPI_BIN=... CAPTURE_RATE=20000 scripts/bench-ingest.sh 10000 600   # V9 sidecar RSS < 256 MB
just fuzz-dpi 600                      # V10
just docker-dpi                        # T077 image build + base-image check
```
