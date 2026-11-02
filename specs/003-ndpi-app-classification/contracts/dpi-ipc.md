# Contract: tmon ↔ classifier IPC, protocol version 1

This contract is public (FR-027): any classifier that implements it can replace `tmon-dpi`.

## Transport

- **Socket**: tmon creates an `AF_UNIX`, `SOCK_SEQPACKET` socketpair and starts the classifier with its end as **file descriptor 3**.
  - One datagram is one message. A message is never split or merged.
  - The maximum message size is **9,400 bytes**.
- **Arguments**: tmon passes `--fd 3 --max-flows N --max-packets-per-conn N --idle-timeout-ms N --memory-limit-mb N`. The environment is cleared, except `RUST_LOG` and `TZ`.
- **Shutdown**: the classifier exits with status 0 when the socket is closed (EOF) or on SIGTERM. tmon sends SIGKILL after 3 s.
- **Logs**: the classifier writes to stderr. tmon forwards the lines to its own log at `debug` level, prefixed `tmon-dpi:`, and rate-limits them to 20 lines/s.

## Encoding

- All integers are little-endian.
- **`str8`**: `u8` length + bytes. ASCII 0x20–0x7E only, length ≤ 64 unless stated otherwise.
- **`ip`**: `u8` family (4 or 6) + 4 or 16 bytes.
- Each message is `u8 type` + body. **Unknown types are ignored** (forward compatibility). A message that is truncated, too long, has an invalid enum value or an invalid string is dropped and counted as `ipc_invalid`.

### Key (both directions)

`lan_ip: ip, lan_port: u16, remote_ip: ip, remote_port: u16, proto: u8`. Both IPs must be the same family.

## tmon → classifier

| Type | Name | Body |
|---|---|---|
| `0x01` | Packet | `key`, `dir: u8` (0 = LAN→remote, 1 = remote→LAN), `ts_ms: u64`, `len: u16`, `l3: [u8; len]` (an IPv4 or IPv6 packet starting at the IP header; `len` ≤ 9,280) |
| `0x02` | Forget | `key` (the connection ended or was evicted on the tmon side; the classifier may free its state) |
| `0x03` | Ping | `nonce: u64` |

## Classifier → tmon

| Type | Name | Body |
|---|---|---|
| `0x81` | Hello | `proto_version: u16` (= 1), `impl: str8` (e.g. `tmon-dpi`), `impl_version: str8`, `engine: str8` (e.g. `nDPI 6.0`), `max_flows: u32`, `catalog_count: u16`, then `catalog_count × (app: str8, category: str8)`. The catalog may be split across several Hello-continuation messages (`0x84`, same body without the first five fields) |
| `0x82` | Classification | `key`, `app: str8` (≤ 48), `category: str8` (≤ 48, may be empty), `confidence: u8` (0 = guess, 1 = confident), `final: u8` (0/1), `host: str8` (may be empty, ≤ 253 via length `u8`) |
| `0x83` | Stats | `uptime_ms: u64`, `packets_in: u64`, `packets_invalid: u64`, `flows_active: u32`, `flows_created: u64`, `evicted_lru: u64`, `evicted_idle: u64`, `evicted_memory: u64`, `classified_confident: u64`, `classified_guess: u64`, `giveups: u64`, `mem_bytes: u64`, `mem_limit_bytes: u64`, `top_count: u8` (≤ 16), then `top_count × (app: str8, flows: u32)` |
| `0x84` | HelloCatalog | `catalog_count: u16`, then entries as in Hello |
| `0x85` | Pong | `nonce: u64` |

## Sequence

1. The classifier initializes, applies its sandbox, then sends **Hello** (and any **HelloCatalog**) within **5 s** of start.
   - If Hello does not arrive in time → tmon kills the classifier and counts a start failure.
   - If `proto_version ≠ 1` → state `incompatible`. tmon kills the classifier and does not restart it.
2. tmon sends Packets and Forgets. The classifier sends **Classification** whenever the result for a key changes. It never sends a guess after a confident result for the same key, and it sends `final = 1` at most once per key.
3. The classifier sends **Stats** every **2 s**; this is the heartbeat. If no Stats arrive for 6 s, tmon restarts the classifier.
4. **Back-pressure**: neither side blocks on the socket.
   - tmon drops Packets when its queue or the socket buffer is full (`dpi_dropped_queue`).
   - The classifier drops Classifications when its send buffer is full (`classifications_dropped`, reported in Stats as part of `packets_invalid`; reserved for v2).

## Limits (enforced on decode)

| Item | Limit |
|---|---|
| Message size | ≤ 9,400 B |
| `l3` length | 20 ≤ len ≤ 9,280 |
| Catalog entries total | ≤ 2,048 |
| `top_count` | ≤ 16 |
| Classifications per second accepted by tmon | ≤ 50,000 (excess dropped and counted) |
