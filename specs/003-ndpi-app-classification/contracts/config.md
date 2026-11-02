# Contract: `[dpi]` configuration (feature 003)

```toml
[dpi]
# Optional application classification (nDPI sidecar). Requires [capture] enabled = true.
enabled = false
binary = "tmon-dpi"              # name looked up in PATH, or an absolute path
max_flows = 50000                # 1_000..=1_000_000
max_packets_per_conn = 32        # 4..=256; packets forwarded per connection
idle_timeout_secs = 900          # 60..=3600
memory_limit_mb = 192            # 32..=2048; classifier allocator budget (RLIMIT_AS = this + 64)
queue_capacity = 16384           # 64..=1_000_000 packets
queue_max_bytes = 16777216       # 1 MiB..=256 MiB
```

## Validation

Errors are reported at startup as `dpi.<key>: <reason>`, in the same style as `capture.*`. The rules:

- `enabled = true` with `capture.enabled = false` → `dpi.enabled: requires capture.enabled = true`.
- Values outside the ranges above → `dpi.<key>: must be between A and B`.
- An empty `binary` is invalid.
- A binary that does not exist is **not** a config error. tmon starts without classification and raises the `dpi_not_found` warning (spec, US4 scenario 5).

## Environment overrides

Environment variables override the file, as for other sections (for example `TMON_DPI__ENABLED=true`).

## Platform

On a non-Linux host, `enabled = true` → the `not_found` state with the message "application classification is only available on Linux".
