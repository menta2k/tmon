# Contract: Capture input (TZSP over UDP) & `[capture]` configuration

## Inbound UDP (FR-001, FR-002)

| Listener | Default bind | Accepts |
|---|---|---|
| `capture` | `0.0.0.0:37008` (only when `capture.enabled = true`) | TZSP datagrams (below) |

**Sender allow-list**:
- If `capture.allowed_sources` is non-empty, it applies.
- Otherwise `collector.allowed_exporters` applies.
- If both are empty, every sender is accepted, and the startup log says so.

Anything else is dropped and counted as `rejected_source`.

### Accepted datagram

```text
0       1       2       4
+-------+-------+---------------+
| ver=1 | type  | encap (BE16)  |   type: 0 (received tag list) only
+-------+-------+---------------+   encap: 1 (Ethernet) only
| tagged fields …  END (0x01)   |   0x00 = PADDING (1 byte), 0x01 = END,
+-------------------------------+   other = tag, len(u8), value[len]
| Ethernet II frame             |   optional 802.1Q / 802.1ad (≤ 2 tags)
|  IPv4 | IPv6 (+ ext hdrs)     |   IPv4 fragments → unsupported{fragment}
|   TCP | UDP                   |
+-------------------------------+
```

| Condition | Outcome / counter |
|---|---|
| datagram < 4 bytes, missing END, or a tag past the end | `malformed{tzsp}` |
| ver ≠ 1, type ≠ 0 or encap ≠ 1 | `unsupported{tzsp}` / `unsupported{encap}` |
| non-IP EtherType, or IP that is neither TCP nor UDP | `unsupported{l3}` |
| truncated L2/L3/L4 header | `malformed{frame}` |
| datagram > 65,535 bytes | impossible over UDP. The receive buffer is 65,535 bytes |

### What is extracted (by packet)

| Packet | Condition | Extractor |
|---|---|---|
| TCP, any port, client → server payload | first payload bytes start with a TLS record header `0x16 0x03 0x0[0-4]` | TLS ClientHello (with reassembly) |
| TCP, any port | first payload starts with an HTTP method token | HTTP `Host` |
| UDP, dst port 443 (or any port when the long-header version is v1/v2) | long header, Initial type | QUIC Initial |
| UDP, src port 53 | DNS response (QR = 1) | DNS answers → device bindings |
| TCP SYN without ACK | | counted as `tcp_syn_seen` only |

Direction: the client is the side that sent the SYN, or the first payload for TLS/HTTP, or the QUIC Initial with the client DCID. For DNS, the requesting device is the response's destination address.

Capture packets never add to byte or flow counts.

## `tmon.toml` additions

```toml
[capture]                              # optional (FR-001), off by default
enabled = false
listen = "0.0.0.0:37008"
allowed_sources = []                   # empty → falls back to collector.allowed_exporters
socket_recv_buffer_bytes = 4194304
queue_capacity = 16384                 # datagrams; full → dropped_backpressure (FR-015)
conn_ttl_secs = 600                    # FR-009
max_connections = 100000               # FR-014
max_reassembly_bytes_per_conn = 16384
max_reassembly_bytes_total = 67108864
max_device_bindings = 200000
reassembly_timeout_secs = 10
silent_warning_secs = 300              # FR-019
```

Environment overrides follow feature 001: `TMON_CAPTURE__ENABLED=true`, and so on.

**Startup validation** (fail fast, with a clear message naming the key):
- `listen` must parse as a socket address.
- `allowed_sources` entries must be IP addresses.
- Every limit must be > 0.
- `max_reassembly_bytes_per_conn` must be ≤ `max_reassembly_bytes_total`.
- `conn_ttl_secs` must be in 60–3600.

**Bind failure**: tmon keeps running and reports `collector_down:37008` (the existing warning shape). Flow collection is unaffected.

## Pluggability (FR-013)

The capture input is one implementation of `tmon_core::naming::ConnectionNamer`. Another implementation (e.g. an nDPI sidecar) can be registered in `app.rs` without any change to the collector, storage, API or UI contracts.
