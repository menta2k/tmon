# Contract: HTTP API changes (feature 003)

All changes are additive. The envelope, auth and error codes are those of feature 001.

## `GET /flows` and `GET /flows/export.csv`

- **New filter**: `app=<name>` (repeatable). Matching is exact and case-insensitive. Only raw ranges are supported; otherwise the request fails with `unsupported_filter_range`.
- **New row fields**:
  - `app`: string or null;
  - `app_confident`: boolean or null.
- `remote_host_source` may now be `"dpi"`.
- **CSV**: adds the columns `app` and `app_confident` after `remote_host_source`.

## `GET /traffic/breakdown?by=app`

Returns rows `{ key: "<app>", bytes, flows, confident_share }` for the range. Flows without an app are grouped under `key: ""` with `label: "Unclassified"`. A JSON `null` key was avoided because every other breakdown uses string keys; filter on it in the UI by the empty string. Raw data only; a range beyond raw retention fails with `unsupported_filter_range`. The usual filters apply: `host`, `category`, `direction`.

## `GET /hosts/{ip}`

Adds `top_apps: [{ app, bytes, flows }]` (up to 10 rows, raw range only; an empty list when the range is not raw).

## `GET /applications/catalog`

Returns `[{ app, category }]` from the classifier's Hello catalog, sorted by app name. It is used by the rule editor picker, and returns `[]` when DPI is disabled or the classifier is not running.

## Rules (`POST/PUT /rules`, `POST /rules/test`)

`match` accepts:

- `app`: string[], 1–64 entries, each 1–48 printable ASCII characters;
- `app_category`: string[], with the same limits.

Rule tests report `confidence` per matched flow, as `detected` or `inferred` depending on `app_confident`.

## `GET /status`: new `dpi` object

```jsonc
"dpi": {
  "enabled": true,
  "state": "running",            // disabled | not_found | starting | running | restarting | failed | incompatible
  "engine": "nDPI 6.0",          // null until Hello
  "impl_version": "0.3.0",
  "proto_version": 1,
  "uptime_secs": 3600,
  "restarts_10m": 0,
  "restarts_total": 1,
  "error": null,                 // last failure text
  "queue": { "sent": 0, "dropped_queue": 0, "dropped_down": 0, "skipped_final": 0, "drop_share_15m": 0.0 },
  "classifier": { "packets_in": 0, "flows_active": 0, "classified_confident": 0, "classified_guess": 0,
                  "evicted": 0, "mem_bytes": 0, "mem_limit_bytes": 0 },
  "top_apps": [{ "app": "BitTorrent", "flows": 12 }]
}
```

When DPI is disabled, the object is `{"enabled": false}`. The key is always present.

## `warnings[]`: new codes

| Code | When | Message (abridged) |
|---|---|---|
| `dpi_not_found` | the binary is missing or not executable | "Application classification is enabled but `tmon-dpi` was not found at <path>. Install the tmon-dpi package or use the -dpi image." |
| `dpi_incompatible` | `proto_version ≠ 1` | "tmon-dpi speaks protocol v<n>, tmon needs v1. Install matching versions." |
| `dpi_failed` | 5 failures in 10 min | "Application classification stopped after repeated crashes (<last error>). Restart tmon to retry; see logs." |
| `dpi_restarting` | ≥ 2 restarts in 10 min, not yet failed | "The classifier restarted <n> times in 10 min." |
| `dpi_dropping` | drop share > 5 % over 15 min | "The classifier cannot keep up; <x>% of packets were skipped. Raise dpi.queue_capacity or reduce capture." |

## `GET /metrics`

```text
tmon_dpi_up                                (gauge 0/1)
tmon_dpi_restarts_total
tmon_dpi_packets_total{result="sent|dropped_queue|dropped_down|skipped_final"}
tmon_dpi_classified_total{confidence="confident|guess"}
tmon_dpi_flows_active                      (gauge)
tmon_dpi_memory_bytes                      (gauge)
tmon_dpi_evicted_total{reason="lru|idle|memory"}
tmon_dpi_ipc_invalid_total
```

There is no per-app label: the label sets stay bounded. Per-app counts are on `/status` only.

## OpenAPI

`openapi.rs` documents the `DpiStatus` schema, the `app` fields and filters, `by=app`, `/applications/catalog` and the warning codes. The route-scan test stays green.
