# tmon — MikroTik traffic monitor

tmon is a single-user, self-hosted flow monitor for MikroTik RouterOS. It receives **IPFIX / NetFlow v5/v9 / sFlow v5** from your router and works out the direction of each flow (down, up, internal, transit). It then enriches flows with GeoIP/ASN data and router-provided host names, and **categorizes** them (Web, Streaming, Gaming, VPN…) with an ordered, editable rule engine. You get a live dashboard, top talkers, per-device drill-downs and raw flow search with CSV export.

- **One binary.** The Rust backend has the Vue UI embedded and runs as a single process with no external database.
- **Lightweight.** Storage is embedded DuckDB plus hourly zstd Parquet files. The target is about 10k flows/s on 2 cores with under 1 GB of RAM.
- **Private.** It works fully offline. The only outbound call is the optional `tmon geoip-update`.

## Quick start (binary)

```bash
cd frontend && pnpm install && pnpm build
cd ../backend && cargo build --release --features embed-ui
./target/release/tmon hash-password <<< 'your-password'      # copy the hash
export TMON_HTTP__PASSWORD_HASH='<hash>'
./target/release/tmon geoip-update --config ../deploy/tmon.example.toml
./target/release/tmon serve --config ../deploy/tmon.example.toml
```

Then point the router at the tmon host (full guide: [docs/routeros.md](docs/routeros.md); OpenWrt: [docs/openwrt.md](docs/openwrt.md)):

```routeros
/ip traffic-flow set enabled=yes interfaces=WAN active-flow-timeout=1m inactive-flow-timeout=15s
/ip traffic-flow target add dst-address=<tmon-ip> port=2055 version=ipfix
```

Open `http://<tmon-host>:8080` and log in.

### Optional: name connections from router packet capture

By default, host names come from the router's DNS cache only, so most traffic is categorized by port ("guessed"). With `[capture] enabled = true`, the router mirrors the **first ~6 KB of each connection** to tmon over TZSP (UDP 37008). tmon then reads the server name from TLS ClientHellos (including ones split across segments), QUIC v1/v2 Initial packets, HTTP `Host` headers and DNS answers, so hostname rules match with *detected* confidence. Each name shows a small source tag (`TLS`, `QUIC`, `HTTP`, `DNS`, `DNS cache`) in Flows and the host drill-down. The Status page has a *Packet capture* card and warns about silent capture, FastTrack bypass and WSL2 packet loss. Router rules and caveats: [docs/routeros.md → Packet capture](docs/routeros.md#packet-capture-connection-naming).

### Optional: identify applications with nDPI

Capture names connections that carry a server name. BitTorrent, WireGuard/OpenVPN, games, STUN/RTP calls and remote desktop carry none. With `[dpi] enabled = true` (requires capture), tmon passes the captured connection setup to **`tmon-dpi`**, a sandboxed sidecar process built on [nDPI](https://github.com/ntop/nDPI). `tmon-dpi` names the application, and rules can match it (`app`, `app_category`). The application appears in Flows, the host drill-down, the dashboard's *By app* view, CSV and `/metrics`. tmon itself never links nDPI: a crash in the sidecar only restarts the sidecar. Use the `tmon:dpi` image (`--target tmon-dpi`) or `just dpi-build`. Setup, sandbox, sizing, troubleshooting and the LGPL notes: [docs/dpi.md](docs/dpi.md).

## Docker

```bash
docker build -f deploy/Dockerfile -t tmon .
echo -n 'your-password' | docker run --rm -i tmon hash-password > /tmp/h && \
  printf "TMON_HTTP__PASSWORD_HASH='%s'\n" "$(cat /tmp/h)" > deploy/.env
docker compose -f deploy/docker-compose.yml up -d
docker compose -f deploy/docker-compose.yml run --rm tmon geoip-update --config /etc/tmon/tmon.toml
```

For systemd, see `deploy/tmon.service`. It uses `DynamicUser`, `StateDirectory=tmon` and a full hardening set.

## Configuration

`deploy/tmon.example.toml` documents every key. Any key can be overridden as `TMON_<SECTION>__<KEY>`, for example `TMON_HTTP__PASSWORD_HASH` or `TMON_ROUTER__PASSWORD`. Keep secrets in the environment, not in the file.

| Section | Highlights |
|---|---|
| `[http]` | `listen`, `password_hash` (required), optional `api_token` for `/metrics` and scripts |
| `[collector]` | UDP listeners (2055/4739 NetFlow/IPFIX, 6343 sFlow), `allowed_exporters` |
| `[storage]` | `disk_cap_gb`, DuckDB `memory_limit`/`threads` |
| `[retention]` | `raw_days` (7), `rollup_1m_days` (30), `rollup_1h_days` (365) |
| `[geoip]` | DB-IP Lite database paths (filled by `tmon geoip-update`) |
| `[router]` | Optional read-only RouterOS REST integration (DHCP and DNS names) |

Once you save retention, disk cap, networks or router settings in the UI, those values take precedence over the file.

## Storage design

Raw flows land in a hot DuckDB table. Every hour they are rotated into `raw/dt=YYYY-MM-DD/HH.parquet` (zstd). Each batch also updates 1-minute and 1-hour rollups by category, host, geo and port, plus hourly top remotes per host. Queries go to the cheapest source that can answer them. A janitor enforces retention and the disk cap, deleting the oldest raw hours first and showing a warning when it does. Changing rules can be applied to history (**Re-apply**), which rewrites the affected Parquet hours atomically and rebuilds their rollups.

**Why not VictoriaMetrics?** It was evaluated as the primary store and rejected. It cannot keep individual flow records (needed for raw search and CSV). High-cardinality dimensions such as remote IP and port explode its series count. Its labels are immutable, so rule changes could not be re-applied to history. DuckDB with Parquet is lighter (no extra service) and supports all of these. If you run VictoriaMetrics or Prometheus anyway, scrape tmon's bounded `/metrics` endpoint:

```yaml
scrape_configs:
  - job_name: tmon
    bearer_token: <api_token>
    static_configs: [{ targets: ["tmon-host:8080"] }]
```

The exposed metrics are `tmon_flows_received_total{exporter,proto}`, `tmon_flows_dropped_total{reason}`, `tmon_decode_errors_total`, `tmon_template_missing_total`, `tmon_bytes_total{category,direction}` and `tmon_storage_bytes`.

## Development

```bash
cd backend && cargo test --workspace            # unit + pcap-replay integration tests
cd frontend && pnpm test:unit && pnpm test:e2e  # Vitest, Playwright
just coverage                                   # ≥ 80 % lines gate
python3 scripts/mock-routeros.py                # fake router REST API for the integration
tmon replay backend/tests/fixtures/mixed-home.pcap --to 127.0.0.1:2055
```

Specs, the plan and the API contract are in `specs/001-mikrotik-flow-monitor/`. The machine-readable API description is served at `GET /api/v1/openapi.json`.

## Attribution

IP geolocation by [DB-IP](https://db-ip.com), licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). The MaxMind test databases in `backend/tests/fixtures/geo` are MIT/Apache licensed (MaxMind-DB test data).
