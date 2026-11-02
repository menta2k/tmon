# Quickstart & Validation: Connection Naming (feature 002)

Run these scenarios to prove the feature end-to-end. The contracts hold the details: [capture-input](contracts/capture-input.md), [router-capture](contracts/router-capture.md), [http-api](contracts/http-api.md). Entities are in [data-model](data-model.md).

## Prerequisites

- Feature 001 builds and passes (`just test`).
- The fixture pcaps are generated: `cargo run -p tmon-capture --example gen_capture_fixtures --features fixtures` (writes `backend/tests/fixtures/capture/*.pcap`).
- For V8/V9: a MikroTik router on RouterOS v7, and tmon running **natively or in Docker** (not WSL2 mirrored mode, see V6).

Minimal config for local runs:

```toml
[capture]
enabled = true
listen = "127.0.0.1:37008"
```

## Scenarios

| # | Scenario | Steps | Expected |
|---|---|---|---|
| V1 | Disabled = unchanged (SC-007) | Run the feature 001 suite + `scripts/bench-ingest.sh` with no `[capture]` | All pass. Ingest rate within 5 % of the 001 baseline. `/status.capture = {"enabled": false}` |
| V2 | TLS + split ClientHello (US1 #1, #3) | `tmon replay tests/fixtures/capture/tls_split_pq.pcap --to 127.0.0.1:37008`, then `tmon replay tests/fixtures/capture_flows.pcap` | The flow to the fixture server shows `remote_host` = SNI, `remote_host_source = tls`, the category from the hostname rule. `by=rule` shows `confidence = detected` |
| V3 | QUIC v1/v2, reordered CRYPTO (US1 #2) | Replay `quic_v1_rfc.pcap`, `quic_v1_reordered.pcap`, `quic_v2_rfc.pcap` + flows | Each flow is named with `source = quic`. `errors.quic_*` stay 0 |
| V4 | Shared CDN, different devices (US1 #4, US2 #2) | Replay `shared_cdn.pcap` (2 devices, same remote IP, different SNI/DNS) + flows | Each device's flow carries its own name |
| V5 | DNS CNAME chain (US2 #1, #3) | Replay `dns_cname.pcap`, then a non-443 flow from the same device to the resolved IP | `remote_host` = queried name, `source = dns`. A rule matching only the CNAME target applies. `remote_hostname` has the name within 5 s. A flow stored *before* the answer is renamed by the back-fill |
| V6 | WSL large-packet loss (US3 #3) | Replay `wsl_truncated.pcap` (≥ 50 ClientHellos, all datagrams ≤ 1,472 bytes) | Warning `capture_large_packets_lost` with remediation text |
| V7 | Silent / FastTrack (US3 #1, #4) | (a) enable capture with `silent_warning_secs = 5` and send nothing; (b) replay `fasttrack_syn_only.pcap` | (a) `capture_silent`; (b) `capture_fastpath_suspected`. The Status card links to the guide |
| V8 | Router setup (SC-006) | Apply [router-capture.md](contracts/router-capture.md) §1–2, then browse from a LAN device | Within 15 min: Status counters move, names by source > 0, no capture warnings. Flows show `tls`/`quic` tags |
| V9 | Coverage on real traffic (SC-001, SC-002) | After 1 h of normal use: `GET /traffic/breakdown?by=rule` and a query of bytes on port 443 with `remote_host` set | ≥ 80 % of port-443 bytes named. "Guessed" + "none" < 40 % of bytes |
| V10 | Safety and bounds (US4, SC-005) | (a) send from a non-allowed IP; (b) `cargo fuzz run capture_datagram -- -max_total_time=600` for each of the 4 targets; (c) run the flood test `cargo test -p tmon-capture --test table_limits -- --ignored flood` while replaying flows | (a) `rejected_source` increases. (b) No crashes. (c) RSS growth < 64 MB, `evicted` > 0, flow ingest has 0 drops |

Also check:
- `cargo bench -p tmon-capture --bench capture_pipeline`: ≥ 50k datagrams/s.
- The coverage gate stays ≥ 80 %.

## Recording results

Record V1–V10 results in `specs/002-connection-naming/validation.md`, using the same format as feature 001. V8 and V9 need the real router.
