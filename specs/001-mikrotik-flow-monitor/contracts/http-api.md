# Contract: HTTP API v1

Base path: `/api/v1`. JSON only. Every endpoint except `POST /auth/login` and `GET /health` requires the session cookie `tmon_session` (or the header `Authorization: Bearer <api_token>`). The authoritative machine-readable form is `GET /api/v1/openapi.json`, generated from code, and it MUST match this document.

## Envelope

```json
{ "ok": true,  "data": { ... }, "error": null, "meta": { "source": "rollup_1m|rollup_1h|raw", "sampled": false, "page": 1, "limit": 100, "total": 1234 } }
{ "ok": false, "data": null, "error": { "code": "validation_failed", "message": "remote_cidr[0]: invalid CIDR", "fields": {"remote_cidr[0]": "invalid CIDR"} } }
```

Error codes and statuses:

| Code | Status |
|---|---|
| `unauthorized` | 401 |
| `validation_failed` | 400 |
| `not_found` | 404 |
| `conflict` | 409 |
| `unsupported_filter_range` | 422 (filter combination only available in raw tier, and the range exceeds raw retention) |
| `rate_limited` | 429 |
| `internal` | 500 (no internals leaked) |

## Common query parameters (analytics endpoints)

| Param | Type | Default | Notes |
|---|---|---|---|
| `from`, `to` | RFC3339 or relative (`now-24h`) | `now-15m`, `now` | `from < to`, maximum span 400 d |
| `step` | duration | auto (about 200 points) | Rounded to the source grain |
| `host` | IP, repeatable | | Local host |
| `category` | id, repeatable | | |
| `proto` | u8, repeatable | | |
| `direction` | `down\|up\|internal\|transit`, repeatable | | |
| `country` | ISO2, repeatable | | |
| `asn` | u32, repeatable | | |
| `exporter` | id, repeatable | | |
| `limit` | 1–1000 | 20 | For top-N endpoints |

Unknown parameters result in a 400 error.

## Endpoints

### Auth / system

| Method | Path | Body / result |
|---|---|---|
| POST | `/auth/login` | `{password}` sets the cookie. Limited to 5 attempts per minute per IP |
| POST | `/auth/logout` | |
| GET | `/health` | `{status:"ok"\|"degraded", checks:{storage, geoip, collectors}}` (no auth) |
| GET | `/status` | `{exporters:[ExporterStatus], collectors:[{proto, port, listening}], ingest:{flows_per_sec, dropped_total{reason}}, storage:{disk_bytes, cap_bytes, oldest_raw}, enrichment:{geoip_db_date, router_integration:{enabled, last_ok, error?}}, warnings:[{code, message}]}` |
| GET | `/live` | **SSE** stream. Every 5 s it sends `event: tick` with `data: {ts, bytes_per_sec:{down, up}, by_category:[{id, bps}]}`, plus `event: warning` events |

### Analytics (FR-014..FR-017)

| Method | Path | Result `data` |
|---|---|---|
| GET | `/traffic/timeseries?group_by=category\|direction\|proto\|country\|exporter` | `{step, series:[{key, label, points:[[ts, bytes]]}]}` |
| GET | `/traffic/breakdown?by=category\|proto\|direction\|country\|asn\|rule` | `[{key, label, bytes, packets, flows, share}]`; `by=rule` rows add `confidence`: `detected` (hostname), `inferred` (ASN/CIDR/country/device), `guessed` (port/protocol only) or `none` (key `0`, no rule matched) |
| GET | `/top/hosts` | `[{ip, name, bytes_down, bytes_up, flows, top_category}]` |
| GET | `/top/remotes` | `[{ip, hostname?, country?, asn?, as_name?, bytes, flows}]` |
| GET | `/top/asns`, `/top/countries`, `/top/ports` | `[{key, label, bytes, flows}]` |
| GET | `/hosts/{ip}` | `{host: LocalHost, totals, categories:[…], protocols:[…], top_remotes:[…20]}` |

### Raw flows (FR-018)

| Method | Path | Notes |
|---|---|---|
| GET | `/flows?…common…&remote_ip=&port=&page=&limit=&sort=&order=` | Newest first by default; `sort=time\|bytes\|packets`, `order=desc\|asc` (also honoured by `export.csv`); `limit` ≤ 500, range ≤ raw retention |
| GET | `/flows/export.csv?…` | Streams `text/csv` with at most 100,000 rows; if truncated, the last line is `# truncated at 100000 rows` |

### Configuration (FR-011..FR-013, FR-020..FR-026)

| Method | Path | Notes |
|---|---|---|
| GET/POST | `/categories` | |
| PATCH/DELETE | `/categories/{id}` | Deleting a built-in category or "Other" returns 409 |
| GET/POST | `/rules` | `Rule` per data-model. Each item in a GET includes `hits_24h` |
| PATCH/DELETE | `/rules/{id}` | |
| POST | `/rules/reorder` | `{ids:[…]}` sets priorities for user rules |
| POST | `/rules/test` | `{flow: partial FlowRecord}` returns `{category_id, rule_id}` (a dry run used by the UI) |
| POST | `/rules/reapply` | `{from?, to?}` starts a background job and returns `{job_id}` |
| GET | `/jobs/{id}` | `{state:"queued\|running\|done\|failed", progress:0..1, error?}` |
| GET | `/hosts` | Paging |
| PATCH | `/hosts/{ip}` | `{friendly_name?, mac?}` |
| GET/PUT | `/networks` | Full replace, `[{cidr, label}]` |
| GET | `/exporters` | |
| PATCH | `/exporters/{id}` | `{name?, sampling_override?, count_direction?}` |
| GET/PATCH | `/settings` | Retention, disk cap, router integration (the password is write-only and never returned) |
| GET | `/settings/export` | JSON bundle of categories, rules, hosts, networks and settings (no secrets) |
| POST | `/settings/import` | Same bundle. Validated in full before it is applied atomically |

## Other interfaces

- `GET /metrics` (Prometheus text, optional token): `tmon_flows_received_total{exporter,proto}`, `tmon_flows_dropped_total{reason}`, `tmon_decode_errors_total`, `tmon_template_missing_total`, `tmon_bytes_total{category,direction}`, `tmon_storage_bytes`. Every label set is bounded.
- `GET /*` serves the embedded Vue SPA, with the history-mode fallback going to `index.html`.
