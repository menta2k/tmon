# Validation results: quickstart V1–V15

**Date**: 2026-11-01
**Build**: release, `--features tmon/embed-ui`, on WSL2 (12 vCPU, 30 GB RAM)
**Fixtures**: synthetic, generated deterministically by `cargo run -p tmon-collector --example gen_fixtures` (see `backend/tests/fixtures/README.md`). GeoIP uses the MaxMind test databases in `backend/tests/fixtures/geo/`.

| # | Scenario | Result | Evidence |
|---|---|---|---|
| V1 | Server refuses to start without `password_hash` | ✅ PASS | `tmon serve` exits 1 with `set http.password_hash (generate one with \`tmon hash-password\`)`. The message wording is slightly more explicit than quickstart's |
| V2 | Ingest IPFIX → dashboard within 30 s | ✅ PASS | `tests/us1_dashboard.rs` (3 tests); Playwright `dashboard.spec.ts` (data shows in ~11 s, time-range switch works) |
| V3 | NetFlow v9 data before template | ✅ PASS | `tmon-collector/tests/netflow_decode.rs`, `tmon-api/tests/status_api.rs`: `template_missing > 0`, no crash, later records decode |
| V4 | sFlow 1:512 sampling | ✅ PASS | `us1_dashboard::sampled_sflow_sets_meta_sampled` (bytes ×512, `meta.sampled`); Playwright shows the "estimated (sampled)" badge |
| V5 | Categorization matches ground truth | ✅ PASS | `us1_dashboard::category_breakdown_matches_ground_truth`: per-category bytes match `mixed-home.truth.csv` exactly (tolerance 1 %) |
| V6 | Top talkers + drill-down | ✅ PASS | `tests/us2_top.rs` (3 tests); Playwright `top-hosts.spec.ts` shows country flag + `AS29518`, and "Local/Private" |
| V7 | Custom rule UDP 27015-27030 → Gaming | ✅ PASS | Playwright `rules.spec.ts` (dashboard shows Gaming, `hits_24h > 0`); `tmon-api/tests/rules_api.rs` |
| V8 | Re-apply rules to history | ✅ PASS | Playwright `rules.spec.ts` (job reaches `done`); `tmon-store/tests/recategorize.rs` (raw Parquet and rollups both rewritten) |
| V9 | Router integration (DHCP/DNS) | ✅ PASS | `tests/us4_router.rs` (wiremock RouterOS): lease host names, DNS remote host names, hostname rule; unreachable router → `router_unreachable` warning while ingest continues. `tmon-enrich/tests/routeros.rs` |
| V10 | Stall warning banner | ✅ PASS | Playwright `status.spec.ts` with `TMON_TEST_STALL_SECS=10`: banner "No flows received from … for N min"; `tmon-collector` health unit tests |
| V11 | Raw search & CSV export | ✅ PASS | Playwright `flows.spec.ts` (host+port filter, paging, CSV with header); `tmon-api/tests/flows_api.rs` |
| V12 | Retention / disk cap | ✅ PASS | `tmon-store/tests/retention.rs`: expired raw hours and rollup rows removed, disk cap drops oldest raw first and flags it |
| V13 | Throughput ≥ 10k flows/s for 10 min | ✅ PASS | `scripts/bench-ingest.sh 11000 600` in a `systemd-run` 2 GB / 200 % CPU cgroup: 6,549,400 flows at 10,916 flows/s, all ingested, `dropped_total{reason="backpressure"}` = 0, peak RSS 357 MB. A 10k-target run achieved 9,923/s (generator pacing), also 0 drops at 346 MB |
| V14 | 30-day dashboard < 2 s, 24 h raw page < 3 s | ✅ PASS | `scripts/bench-queries.sh` (30 d at 10k flows/h seeded by `seed_rollups`): slowest 30 d endpoint `top/remotes` 53 ms cold; 24 h raw first page 12 ms |
| V15 | Accuracy vs router interface counters (< 2 %) | ⏸ PENDING | Needs the real MikroTik router. Compare the 1 h WAN total with `/interface print stats` delta |

## Automated suites

| Suite | Result |
|---|---|
| Backend `cargo test --workspace` | 130 passed, 0 failed |
| Backend coverage (`cargo llvm-cov --workspace`) | **86.1 % lines** (82.6 % regions, 84.5 % functions), gate ≥ 80 % ✅ |
| Frontend unit (`vitest --coverage`) | 159 passed; **94.9 % lines** ✅ |
| Frontend lint + type-check | clean |
| Playwright e2e (8 tests) | 8 passed |
| Criterion `rules` | 51 ns/flow with built-in rules (target < 1 µs) ✅ |
| Criterion `pipeline` | 6.9 M flows/s classify+geo+categorize (target ≥ 100 k/s/core) ✅ |
| cargo-fuzz `netflow`, 10 min | 7.7 M runs, 0 crashes (after fixes below) ✅ |
| cargo-fuzz `sflow`, 10 min | 1.3 M runs, 0 crashes ✅ |

## Defects found during validation (all fixed)

- **Fuzz: arithmetic overflow in NetFlow decoding.**
  - Summing flow bytes panicked on huge counters. Huge counters could also overflow `sum(bytes)` in the DuckDB rollups and fail a whole ingest batch.
  - Fix: per-flow bytes and packets are now clamped to 2⁴⁰ (`fields::MAX_FLOW_COUNT`), and sums saturate.
  - Fuzz: extreme `sysUpTime` values overflowed the time arithmetic. Fix: saturating maths in `Fields::times` and `fix_skew`.
  - Regression inputs: `tmon-collector/tests/regress/*.bin`.
- **Security review (T117).**
  - `/metrics` was anonymous when no `api_token` was set. Fix: it now needs a session or the token.
  - The exporter registry was unbounded (spoofed sources or domain ids). Fix: capped at 256, rejects counted as `exporter_limit`.
  - Successful logins counted toward the login throttle. Fix: only failed attempts count now.
  - `POST /settings/router/test` could send the stored router password to a new URL. Fix: the password is now required when the url or username changes.
  - CSV formula injection in free-text columns. Fix: those cells are now prefixed with `'`.
  - CSP gained `object-src 'none'; base-uri 'none'; form-action 'self'`.
- **Rule `hits_24h` cache.** The cache lived 5 min, so a newly created rule showed 0 hits after traffic. Fix: the cache now lives 15 s.
- **UI "Local/Private" label.** The UI did not treat RFC 5737 / `2001:db8::/32` documentation ranges as private, while the backend did. Both now agree.
- **e2e harness.** sFlow fixtures were replayed to the NetFlow port, and the Naive UI pagination disabled state was detected wrongly. Both fixed.

## Deviations from plan

- Timestamps are stored as BIGINT epoch ms and IPs as VARCHAR in DuckDB (simpler Parquet round-trips and filters).
- The OpenAPI document is hand-built (`tmon-api/src/openapi.rs`) instead of generated by utoipa. `tests/openapi.rs` asserts that every route is documented.
- Fixtures are synthetic (generated) rather than captured from a router. Replace or add real captures with `tcpdump` (see `scripts/gen-fixtures.md`).
- Rollups are maintained incrementally inside the ingest transaction (`duck/ingest.rs`), with no separate rollup job.
- The dev profile builds with `debug = "line-tables-only"` (dependencies: no debug info). DuckDB is statically linked into every test binary, and with full debug info the dev build cache grew to about 245 GB.
