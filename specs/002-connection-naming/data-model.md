# Data Model: Connection Naming from Router Packet Capture

Almost everything here is **in memory only** (FR-016). The only persisted change is one nullable flow column. Decisions are referenced as R-numbers from [research.md](research.md).

## 1. Core types (`tmon-core/src/naming.rs`)

### ConnKey

The LAN-side connection identity (FR-009, R8). It is built from the client → server packet, and from a `FlowRecord` after un-NAT.

| Field | Type | Notes |
|---|---|---|
| `lan_ip` | `IpAddr` | LAN device (client) |
| `lan_port` | `u16` | client port |
| `remote_ip` | `IpAddr` | server |
| `remote_port` | `u16` | server port |
| `proto` | `u8` | 6 (TCP) or 17 (UDP) |

- `ConnKey::from_record(&FlowRecord)` maps `local_* → lan_*` and `remote_* → remote_*`.
- It derives `Hash`, `Eq` and `Copy`.

### NameSource

A `#[repr(u8)]` enum. It is persisted as `remote_host_source` (R11).

| Variant | Code | API string | Origin |
|---|---|---|---|
| `Tls` | 1 | `tls` | TLS ClientHello SNI |
| `Quic` | 2 | `quic` | QUIC Initial SNI |
| `Http` | 3 | `http` | plain HTTP Host |
| `Dns` | 4 | `dns` | captured DNS answer (device binding) |
| `Router` | 5 | `router` | router DNS-cache poll (feature 001) |

`NameSource::from_code(u8) -> Option<Self>`: unknown codes give `None`, so the API reports `null`.

### ConnName

Immutable once published. It is shared as `Arc<ConnName>`.

| Field | Type | Validation |
|---|---|---|
| `host` | `Arc<str>` | normalized: lowercase ASCII, trailing `.` stripped, 1–253 bytes, labels 1–63 bytes of `[a-z0-9_-]` (IDNA A-labels allowed). Otherwise rejected and counted as `name_invalid` |
| `aliases` | `Arc<[Arc<str>]>` | CNAME chain targets (DNS only). Same validation. At most 8 |
| `alpn` | `Option<Arc<str>>` | first offered ALPN (`h2`, `h3`, `http/1.1`…), at most 32 bytes, printable ASCII |
| `source` | `NameSource` | |
| `ech_outer` | `bool` | true when the ClientHello carried `encrypted_client_hello` (FR-008). The name is then **not** used as `remote_host` (see lookup) |

### trait ConnectionNamer (FR-013)

```rust
pub trait ConnectionNamer: Send + Sync {
    /// Exact connection first, then the device binding (lan_ip, remote_ip).
    fn lookup(&self, key: &ConnKey, now_ms: i64) -> Option<Arc<ConnName>>;
}
```

## 2. FlowRecord additions (`tmon-core/src/flow.rs`)

| Field | Type | Persisted | Notes |
|---|---|---|---|
| `remote_host_source` | `Option<NameSource>` | yes (`UTINYINT`, nullable) | set together with `remote_host` |
| `remote_aliases` | `Option<Arc<[Arc<str>]>>` | **no** | used only by the hostname matcher at ingest (R11a) |

- Builders: `with_remote_name(host, source)` and `with_aliases(aliases)` return new records (immutability rule).
- The matcher (`rules/matcher.rs::hostname_matches`) checks `remote_host`, then each alias.

### Enrichment lookup order (FR-010)

1. Exact `ConnTable` entry. If `ech_outer`, skip to step 2 but remember the ECH name as a last resort.
2. `DeviceBindings[(lan_ip, remote_ip)]`, giving source `Dns` and its aliases.
3. The router DNS-cache map (`RemoteNames`, existing), giving source `Router`.
4. The ECH outer name, if nothing else matched, with source `Tls`/`Quic`. It is still "detected" as its front domain, so rules for the provider apply.

## 3. Capture state (`tmon-capture`)

### Where state lives (as implemented)

The design above had one `TrackedConnection` per key inside the shared `ConnTable`. The implementation splits it so that payload bytes never leave the single processor task:

- **`CaptureProcessor`** (`processor/mod.rs`, one task, no locks) owns all per-connection *work* state:
  - `syns: LRU<ConnKey, isn>`: SYN seen, no payload yet;
  - `pending: LRU<ConnKey, Pending>`: reassembly in progress (`Pending::Tcp { reasm, kind, started_ms }` or `Pending::Quic { keys, reasm, started_ms }`);
  - `done: LRU<ConnKey, Done { at_ms, isn }>`: named or given up. Later packets are ignored until a **new SYN with a different ISN** reopens the tuple (a retransmitted SYN does not), or until `conn_ttl` has passed.
  - Each LRU holds at most `max_connections` entries.
- **`ConnTable`** (`table.rs`, 16 sharded mutex LRUs) holds only results: `ConnKey → (Arc<ConnName>, last_used_ms)` and the device bindings. The flow pipeline reads it through `ConnectionNamer`, using the host clock for TTL decisions.

State transitions:

```text
(SYN) ──► syns ──first client payload / QUIC Initial──► pending
pending ──complete ClientHello / Host──► done + ConnTable.insert(name)
pending ──cap exceeded / parse error / 10 s timeout / LRU or global byte-cap eviction──► done (counted)
done ──new SYN (other ISN) or conn_ttl elapsed──► forgotten
```

`done` and `ConnTable` entries hold no payload bytes. A `pending` entry holds at most `max_reassembly_bytes_per_conn` bytes.

### Reassembly (`tcp_reasm.rs`, shared by TCP and QUIC CRYPTO)

- **Contents**: `TcpReassembly { base_seq, prefix: Vec<u8>, pending: BTreeMap<u32 offset, Vec<u8>>, cap }`.
- **Complete when** the contiguous prefix covers the TLS record (5 + length), the HTTP headers, or the QUIC handshake message (4 + length).
- **Overlaps and duplicates** are trimmed against the prefix. Out-of-order segments are kept only while `bytes() + segment ≤ cap` and while there are fewer than 16 of them. Otherwise the connection fails as `Overflow`.
- **Global cap**: the processor tracks the total across all `pending` entries. Over `max_reassembly_bytes_total`, it evicts the least recently used pending entries (`conn_evicted{bytes}`).

### DeviceBinding (in `DeviceBindings`)

| Field | Type | Notes |
|---|---|---|
| key | `(lan_ip: IpAddr, remote_ip: IpAddr)` | the response's destination IP and the answer address |
| `name` | `Arc<ConnName>` | `host` = the queried name, `aliases` = the CNAME chain, `source = Dns` |
| `expires_ms` | `i64` | `now + clamp(ttl, 10 min, 1 h)` (R6) |

- Capacity is `max_device_bindings` (default 200k), with LRU eviction.
- A newer answer for the same key replaces the old binding (insert-replace, no mutation).

### PendingRemoteNames

A small buffer of `(remote_ip, host)` from DNS answers. Every 5 s it is drained into the store's `remote_hostname` upsert, and `backfill_hostnames` (feature 001) runs (FR-012). Duplicates are collapsed. At most 10k per drain.

## 4. Health and counters (`tmon-capture/src/health.rs`)

These are all monotonically increasing `AtomicU64`, except where marked as a gauge. Snapshots are immutable `CaptureStats` values.

| Counter | Meaning |
|---|---|
| `datagrams_received`, `bytes_received` | accepted UDP datagrams |
| `rejected_source` | sender not on the allow-list (FR-002) |
| `dropped_backpressure` | channel full (FR-015) |
| `unsupported{tzsp, encap, l3, fragment}` | not decodable by design (FR-003) |
| `malformed{tzsp, frame}` | truncated or invalid headers |
| `tls_started`, `tls_named`, `tls_incomplete`, `tls_parse_error`, `tls_ech` | |
| `quic_started`, `quic_named`, `quic_unsupported_version`, `quic_decrypt_error`, `quic_incomplete` | |
| `http_named` | |
| `dns_responses`, `dns_bindings`, `dns_parse_error` | |
| `name_invalid` | rejected by name validation |
| `tcp_syn_seen`, `tcp_payload_first_seen` | FastTrack detector |
| `conn_evicted{lru, ttl, bytes_cap}`, `bindings_evicted` | |
| `conns_tracked`, `bindings_tracked`, `reassembly_bytes` | gauges |
| `max_datagram_len` (15-min window) | WSL detector |
| `last_datagram_ms` | silence detector |
| `lookups`, `lookup_hits{conn, dns}` | gives the share of flows named (FR-018) |

The 15-minute windowed detectors keep 15 one-minute buckets in a ring (no unbounded history). Thresholds are in [research.md](research.md) R10 and are `const`s in `health.rs`.

## 5. Storage (`tmon-store`)

| Change | Detail |
|---|---|
| Migration 6 | `ALTER TABLE flows_hot ADD COLUMN IF NOT EXISTS remote_host_source UTINYINT` |
| Parquet writer | the column is written in new hourly files |
| Readers | `read_parquet(..., union_by_name = true)` in `files.rs` (flows view), and in `recategorize.rs` (rewrite + rebuild), so old files read as NULL |
| Rollups | unchanged (no new dimension) |
| Raw flow API row | `remote_host_source: "tls"\|"quic"\|"http"\|"dns"\|"router"\|null` |

Router DNS-cache names applied at ingest now also set `remote_host_source = 5`. Back-fill (feature 001) sets 4 or 5 depending on where the name came from. Older rows stay NULL.

## 6. Configuration entity

The `[capture]` section is in [contracts/capture-input.md](contracts/capture-input.md). It is validated at startup (fail fast). Limits must be > 0, and `max_reassembly_bytes_per_conn` must be ≤ `max_reassembly_bytes_total`.
