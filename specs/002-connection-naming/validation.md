# Validation results: quickstart V1–V10 (feature 002)

**Date**: 2026-11-01
**Build**: release, `--features tmon/embed-ui`, on WSL2 (12 vCPU, 30 GB RAM)
**Fixtures**: synthetic, generated deterministically by `just fixtures-capture` (`tmon-capture/examples/gen_capture_fixtures.rs`) into `backend/tests/fixtures/capture/`. The QUIC vectors are the RFC 9001 / RFC 9369 Appendix A client Initials.

| # | Scenario | Result | Evidence |
|---|---|---|---|
| V1 | Disabled = unchanged | ✅ PASS | `us7_capture::disabled_capture_changes_nothing`: `/status.capture == {"enabled": false}`, no `capture_*` warnings, `mixed-home` totals exact, `remote_host_source` only NULL/`router`. Feature 001 suites green. Ingest bench `scripts/bench-ingest.sh 10000 60` (2 GB / 200 % cgroup): baseline 594,600 flows (9,909/s), 0 drops, peak RSS 210 MB; with `CAPTURE=1` idle 595,000 flows (9,915/s), 0 drops, 207 MB, so it is within 5 % |
| V2 | TLS + split post-quantum ClientHello | ✅ PASS | `us7_capture::captured_handshakes_and_dns_name_flows` (`tls_split_pq`: `rr3.googlevideo.com`, `tls`, Streaming); `tmon-capture/tests/tls.rs` (8, incl. seq wrap, reordering, ECH); `by=rule` shows ≥ 4 `detected` |
| V3 | QUIC v1/v2, reordered CRYPTO | ✅ PASS | Same e2e test (`quic_v1_rfc`, `quic_v1_reordered` → `chatgpt.com`/AI Services, `quic_v2_rfc`), `quic_decrypt_error = 0`; `tests/rfc9001_vectors.rs` (6), `tests/quic_reorder.rs` (5) |
| V4 | Shared CDN, different devices | ✅ PASS | `shared_cdn`: `www.netflix.com` → Streaming and `www.reddit.com` → Social on the same remote IP |
| V5 | DNS CNAME chain + back-fill | ✅ PASS | `dns_cname`: `media.example.org` (`dns`) categorized Streaming only through the CNAME target `rr5.googlevideo.com`. `us7_capture::late_dns_answer_backfills_an_earlier_flow` (back-filled via the 5 s name feed, source `dns`) |
| V6 | WSL large-packet loss | ✅ PASS | `us7_capture::truncated_capture_raises_the_large_packet_warning`: 60 incomplete hellos, all datagrams ≤ 1,472 B → `capture_large_packets_lost` (WSL2 text); detector thresholds unit-tested (`tests/health.rs`) |
| V7 | Silent / FastTrack | ✅ PASS | (a) `tmon-api/tests/status_capture.rs::detector_warnings_reach_status` and `tests/health.rs` (silent at > 300 s, also when nothing ever arrived). (b) `us7_capture::syn_only_capture_raises_the_fastpath_warning` (120 SYNs). The Status card and banner link to the guide (`capture.spec.ts`) |
| V8 | Router setup | ⏸ PENDING | Needs the real MikroTik router, with tmon running natively or in Docker (not WSL2 mirrored, see V6). Apply `docs/routeros.md` → *Packet capture* |
| V9 | Coverage on real traffic (≥ 80 % of 443 bytes named) | ⏸ PENDING | Needs 1 h of real traffic through the router (after V8) |
| V10 | Safety and bounds | ✅ PASS | (a) `tests/listener.rs`: non-allowed sender → `rejected_source`, never decoded. (b) Fuzzing, see below: 0 crashes. (c) `us7_capture::capture_flood_does_not_drop_flows` (200k noise datagrams during the `mixed-home` replay: 0 flow drops, tables within caps); `CAPTURE=1 CAPTURE_RATE=20000 scripts/bench-ingest.sh 10000 60`: 1.2 M capture datagrams (480k connections) alongside 595,100 flows; 0 flow drops, 0 capture drops, the table is held at its 100,000 cap, and peak RSS is 257 MB, **+47 MB** over the baseline (limit 64 MB) ✅ |

## Automated suites

| Suite | Result |
|---|---|
| Backend `cargo test --workspace` | 255 passed, 0 failed; `cargo fmt --check` and `cargo lint` clean |
| Backend coverage (`cargo llvm-cov --workspace`) | **85.3 % lines** (87.3 % functions); `tmon-capture` files 69–100 %; gate ≥ 80 % ✅ |
| Frontend unit (`vitest --coverage`) | 178 passed; **95.8 % lines** ✅ |
| Frontend lint + type-check | clean |
| Playwright e2e (8 tests, against a release build with `[capture]` enabled) | 8 passed |
| Criterion `capture_pipeline` (TLS-PQ/QUIC/HTTP/DNS mix, 4,000 distinct connections, all named) | **~780 k datagrams/s on one core** (target ≥ 50 k) ✅ |
| cargo-fuzz, 5 min each (seeded from the fixtures) | `capture_datagram` 2.7 M, `tls_client_hello` 22 M, `quic_initial` 3.6 M, `dns_response` 5.6 M runs; 0 crashes ✅ |
| cargo-fuzz, 10 min each | `capture_datagram` 4.1 M, `tls_client_hello` 30.5 M, `quic_initial` 4.5 M, `dns_response` 9.3 M runs; 0 crashes ✅. After the review fixes, `capture_datagram` re-ran for 10 min: 3.8 M runs, 0 crashes ✅. Regression harness: `tmon-capture/tests/fuzz_regressions.rs` |

## Defects found during review (all fixed)

- **Code review H1: reused 4-tuple never named again.** The processor's `done` set had no TTL and no reset on a new SYN. Fix: entries carry `at_ms` + ISN, expire after `conn_ttl`, and a SYN with a different ISN reopens the tuple (a retransmitted SYN does not). Tests: `processor_hardening::a_new_connection_on_a_reused_tuple_is_named_again`, `finished_tuples_expire_after_the_connection_ttl`.
- **Code review H2: per-connection cap bypass.** Many overlapping out-of-order segments were each checked only by offset, so one tuple could hold megabytes. Fix: out-of-order inserts must keep `bytes() ≤ cap`, with at most 16 pending segments. Test: `out_of_order_flood_stays_within_the_connection_cap`.
- **Queue memory not bounded by bytes (both reviews).** Datagrams over 9,280 B are now counted as malformed and never copied. The queue also has a 16 MiB byte budget on top of its slot count.
- **Security: forged DNS answers.**
  - Captured names no longer overwrite router DNS-cache rows in `remote_hostname`.
  - The table is capped at 200k rows (oldest dropped).
  - Answers for loopback, unspecified, multicast and broadcast addresses are ignored.
- **Security: all-numeric host names** (`192.168.1.1`) are rejected by `normalize_host`.
- **Clock skew.** Name lookups used the router's flow timestamp against host-clock TTLs. They now use the host clock.
- **QUIC.** The first Initial was decrypted twice. `quic_initial` is now split into small steps.
- **Config.** Table and queue sizes have upper bounds (5 M entries, 1 GiB reassembly), so a typo cannot OOM at startup.

## Known limitations / follow-ups (not fixed)

- **Open listener.** With an empty `capture.allowed_sources` *and* empty `collector.allowed_exporters`, every sender is accepted (contract behaviour, warned at startup). Source IPs are spoofable, so the guide says to firewall UDP 37008 to the router.
- **Forged DNS answers to a LAN address can still bind per-device names.** There is no query/txid matching. That is acceptable for the threat model (a LAN attacker can already set any SNI).
- **Sweeps.** `ConnTable::sweep` (every 10 s) and the processor's per-second sweep scan whole tables. That is fine at the defaults; for very large limits, batch it.
- A ClientHello fragmented across several TLS *records* (rare) is not reassembled, and an unknown QUIC version on the HTTPS port finishes the tuple.

## Deviations from plan

- Reassembly state lives in the single-threaded `CaptureProcessor`, not in `ConnTable`; only names are shared (data-model.md §3 updated).
- `tls_incomplete` is counted when the 10 s reassembly timeout fires, so the WSL detector reacts within one sweep after the timeout.
- The setup-guide links in the UI render as text unless `VITE_DOCS_BASE_URL` is set at build time (the SPA does not serve `docs/`).
