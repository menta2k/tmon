# Quickstart & Validation Guide: tmon

This guide proves the feature works end to end. For details, see [contracts/config.md](./contracts/config.md) (ports, config, RouterOS commands) and [contracts/http-api.md](./contracts/http-api.md) (endpoints).

## Prerequisites

- Linux x86_64/arm64 host on the router's LAN, 2 cores, 2 GB RAM
- Rust stable (≥ 1.85, edition 2024), Node 22 LTS + pnpm (only to build from source)
- A MikroTik RouterOS v7 router, **or** the pcap fixtures in `backend/tests/fixtures/` together with `tmon replay`

## Build & run

```bash
cd frontend && pnpm install && pnpm build          # outputs to frontend/dist (embedded by backend)
cd ../backend && cargo build --release
./target/release/tmon hash-password <<< 'changeme'  # copy the hash
export TMON_HTTP__PASSWORD_HASH='<hash>'
./target/release/tmon geoip-update
./target/release/tmon serve --config ../deploy/tmon.example.toml
```

Docker alternative: `docker compose -f deploy/docker-compose.yml up -d`. This runs one container with UDP 2055/4739/6343 and TCP 8080 published, and a volume for `/var/lib/tmon`.

## Validation scenarios

| # | Scenario (spec ref) | Steps | Expected |
|---|---|---|---|
| V1 | Server starts securely (FR-024/025) | Start without `password_hash` | Exits non-zero with the message "set http.password_hash (tmon hash-password)" |
| V2 | Ingest IPFIX (US1, FR-001) | `tmon replay tests/fixtures/routeros7-ipfix.pcap --to 127.0.0.1:2055`, then open `http://localhost:8080` and log in | The dashboard shows traffic within 30 s (SC-003). `/api/v1/status` lists exporter `ipfix` with `parse_errors=0` |
| V3 | NetFlow v9 template-before-data (US5) | Replay `nf9-data-before-template.pcap` | `template_missing` > 0, no crash, and later records decode |
| V4 | sFlow sampling (FR-002/004) | Replay `sflow-crs-1in512.pcap` | Bytes are scaled ×512 and the UI shows the "estimated (sampled)" badge |
| V5 | Categorization (US1, FR-009) | Replay `mixed-home.pcap` (has a known ground-truth CSV) | Per-category bytes match the ground truth within 1 %. "Other" is ≤ 20 % of bytes (SC-006 with hostnames) |
| V6 | Top talkers + drill-down (US2) | Open Top Hosts for the fixture range and click the first host | The ranking matches the ground truth. Top remotes show country/ASN, and private IPs show "Local/Private" |
| V7 | Custom rule (US3) | Add rule `UDP 27015-27030 → Gaming`, replay `steam.pcap` | Traffic appears under Gaming, and the rule's `hits_24h` > 0 |
| V8 | Re-apply rules (FR-023) | After V7, run `POST /rules/reapply` for the fixture range and poll the job | Job is `done`. Historical bytes move to Gaming in both the raw and rollup views |
| V9 | Router integration (US4) | Run `scripts/mock-routeros.py` (serves `/rest/ip/dhcp-server/lease` and `/rest/ip/dns/cache`) and enable `[router]` | Host names and remote hostnames appear. Stopping the mock shows an integration warning while ingest continues |
| V10 | Stall warning (FR-019) | Stop the replay and wait 5 min (or set `TMON_TEST_STALL_SECS=10`) | The banner "No flows received from 127.0.0.1 for 5 min" appears |
| V11 | Raw search & CSV (US6) | Filter `/flows` by host+port, then export | Rows match the filter. The CSV opens with a header row |
| V12 | Retention / disk cap (FR-020/022) | Set `raw_days=1`, inject backdated Parquet files, trigger the janitor | Old `dt=` dirs are removed, and the status page shows the new `oldest_raw` |
| V13 | Throughput (SC-002) | `tmon replay big.pcap --speed max` at ≥ 10k flows/s for 10 min in a 2 CPU / 2 GB cgroup | `dropped_total{reason="backpressure"}` = 0, RSS < 1 GB |
| V14 | Query speed (SC-004) | Load the seeded 30-day rollups (`scripts/seed-rollups`), then open the dashboard at 30 d | Page data loads in < 2 s. A 24 h raw search's first page loads in < 3 s |
| V15 | Accuracy (SC-005) | Real router: compare the 1 h WAN total with `/interface print stats` delta | Difference < 2 % |

## Automated test commands

```bash
cd backend  && cargo test --all && cargo llvm-cov --fail-under-lines 80
cd frontend && pnpm test:unit --coverage && pnpm test:e2e      # Playwright runs V2, V6, V7, V10, V11 against replayed fixtures
```
