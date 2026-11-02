# Contract: HTTP API deltas (feature 002)

All endpoints are unchanged unless listed here. The envelope, auth and error codes are those of [feature 001](../../001-mikrotik-flow-monitor/contracts/http-api.md). Every change is additive, so existing clients keep working.

## `GET /status`: new `capture` object (FR-018)

```jsonc
"capture": {
  "enabled": true,
  "listening": true,
  "address": "0.0.0.0:37008",
  "error": null,                      // bind error text when listening=false
  "last_datagram_at": "2026-11-01T10:00:00Z",   // null if never
  "datagrams": { "received": 0, "rejected_source": 0, "dropped_backpressure": 0,
                 "unsupported": 0, "malformed": 0, "max_len_15m": 1518 },
  "connections": { "tracked": 0, "evicted": 0, "reassembly_bytes": 0 },
  "bindings": { "tracked": 0, "evicted": 0 },
  "names": { "tls": 0, "quic": 0, "http": 0, "dns": 0, "ech": 0 },
  "errors": { "tls_incomplete": 0, "tls_parse": 0, "quic_unsupported_version": 0,
              "quic_decrypt": 0, "quic_incomplete": 0, "dns_parse": 0, "name_invalid": 0 },
  "named_share_15m": 0.83             // lookups that hit / lookups, last 15 min; null if no lookups
}
```

When capture is disabled the object is `{"enabled": false}`. The `capture` key is always present.

## `warnings[]`: new codes (FR-019..FR-021)

| Code | When (thresholds in research R10) | Message (abridged) |
|---|---|---|
| `capture_silent` | enabled, and no datagram for `silent_warning_secs` (or never) | "Capture is enabled but no packets arrived in 5 min. Check the router mangle rules (see setup guide)." |
| `capture_large_packets_lost` | ≥ 50 TLS starts in 15 min, ≥ 50 % incomplete, max datagram ≤ 1,472 | "Large capture packets are being lost (e.g. WSL2 mirrored networking). Run tmon natively or in Docker." |
| `capture_fastpath_suspected` | ≥ 100 SYNs in 15 min, < 10 % with first payload seen | "Connections are seen but their setup packets are not. Exempt the first 6 KB from FastTrack." |
| `collector_down:<port>` | the capture socket failed to bind (existing shape) | |

Warnings are also pushed over the existing SSE `warnings` event.

## `GET /flows` and `GET /flows/export.csv` (FR-022)

- Each row gains `remote_host_source`: `"tls" | "quic" | "http" | "dns" | "router" | null`. It is `null` for flows stored before this feature, or with no name.
- The CSV gains a `remote_host_source` column after `remote_host`.

## `GET /hosts/{ip}` drill-down

The remote rows that already include `hostname` gain `hostname_source` (same values). It is taken from the latest raw flow, or `null` when it comes from rollups only.

## `GET /metrics` (Prometheus)

All metrics are counters unless marked as gauges:

```text
tmon_capture_datagrams_total
tmon_capture_rejected_total
tmon_capture_dropped_total{reason="backpressure"}
tmon_capture_unsupported_total{kind="tzsp|encap|l3|fragment"}
tmon_capture_malformed_total{layer="tzsp|frame"}
tmon_capture_names_total{source="tls|quic|http|dns"}
tmon_capture_errors_total{kind="tls_incomplete|tls_parse|quic_version|quic_decrypt|quic_incomplete|dns_parse|name_invalid"}
tmon_capture_evicted_total{what="conn_lru|conn_ttl|conn_bytes|binding"}
tmon_capture_connections            (gauge)
tmon_capture_bindings               (gauge)
tmon_capture_reassembly_bytes       (gauge)
tmon_capture_lookups_total{result="conn|dns|miss"}
```

## OpenAPI

`openapi.rs` documents the new `capture` schema, the warning codes and `remote_host_source`. The existing route-scan test must stay green.
