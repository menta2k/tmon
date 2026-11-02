# Research: Connection Naming from Router Packet Capture

Each decision lists what was chosen, why, and what else was considered. Library versions were checked on crates.io on 2026-11-01. `ring` and `httparse` are already in `backend/Cargo.lock` (pulled in by rustls and hyper).

## R1. Capture transport: TZSP from RouterOS

- **Decision**: Receive TZSP over UDP (default `0.0.0.0:37008`). Accept version 1, type 0 ("received tag list") and encapsulation 1 (Ethernet). Skip tagged fields until the END tag (`0x01`): PADDING (`0x00`) is a single byte, other tags are TLV (tag, length, value). Everything after END is the Ethernet frame. Count anything else as `unsupported`.
- **Rationale**: RouterOS can stream from firewall rules natively (`action=sniff-tzsp`) without extra software. RouterOS sends no tags beyond END, but TLV skipping keeps other TZSP senders working.
- **Alternatives**:
  - Port mirroring to a host NIC + pcap: needs a managed switch or a second NIC and doesn't fit WSL/Docker setups.
  - `/tool sniffer` streaming: not filterable to the first bytes of each connection, so far more traffic.

## R2. Which packets the router sends (router rules)

- **Decision**: The setup guide adds three mangle rules (exact commands in [contracts/router-capture.md](contracts/router-capture.md)):
  1. `chain=prerouting in-interface-list=LAN connection-bytes=0-6000 action=sniff-tzsp`: client → server setup (TLS ClientHello, QUIC Initial, HTTP request, DNS queries).
  2. `chain=output protocol=udp src-port=53 action=sniff-tzsp`: DNS answers from the router's own resolver to LAN clients.
  3. `chain=prerouting in-interface-list=WAN protocol=udp src-port=53 action=sniff-tzsp`: DNS answers from external resolvers.
- **Rationale**:
  - Only client → server packets carry the names we need, except DNS answers.
  - Answers from the router's resolver are router-originated (`output` chain), so rule 1 never sees them.
  - `connection-bytes=0-6000` limits capture to connection setup (~1–3 Mbit/s at home rates).
- **FastTrack**: FastTracked packets bypass mangle. A ClientHello is sent once the connection is established, so the default `fasttrack-connection connection-state=established,related` rule would skip it. Guidance is to add `connection-bytes=6000-0` to the FastTrack rule so only bytes after the first 6 KB are fast-pathed. Detected at runtime by R10.
- **To verify on the user's router (quickstart V3)**: that `sniff-tzsp` is accepted in `chain=output`, and that the FastTrack change does not raise router CPU noticeably.

## R3. Frame decoding

- **Decision**: `etherparse` 0.21 (MIT/Apache-2.0), using its zero-copy `SlicedPacket` for Ethernet II, single/double VLAN, IPv4, IPv6 (with extension headers), TCP and UDP. IPv4 fragments are counted as `unsupported` (setup messages are never fragmented at IP level in practice).
- **Rationale**: Mature, widely used and fuzzed upstream. It also *builds* packets, which the fixture generator needs.
- **Alternatives**:
  - `pnet_packet`: heavier, less maintained.
  - A hand-written parser: more code to fuzz, with no benefit.

## R4. TLS ClientHello: server name and ALPN

- **Decision**: `tls-parser` 0.12 (MIT/Apache-2.0, nom-based). It parses one TLS record containing a `ClientHello` and reads the `server_name` (0) and `application_layer_protocol_negotiation` (16) extensions. The presence of `encrypted_client_hello` (0xfe0d) marks the visible name as an **ECH outer name** (FR-008): it is the provider's `public_name`, not the destination. Detecting the extension is more robust than a list of front names.
- **TCP reassembly**: Per connection, buffer client → server payload in sequence order, starting at the first payload byte after the handshake (or the first payload segment seen):
  - Stop when the TLS record header's length (5-byte header + `length`) is satisfied.
  - Give up on per-connection cap overflow (16 KB, FR-014), on a gap larger than the cap, or after 10 s.
  - Out-of-order segments are placed by sequence number. Duplicates are ignored.
- **Rationale**: Post-quantum key shares (X25519MLKEM768) make Chrome/Firefox ClientHellos about 1.7–2 KB, which spans 2 TCP segments. Without reassembly most modern browsers would go unnamed.
- **Alternatives**: `rustls` internals (not a public parsing API); hand-parsing (larger fuzz surface).

## R5. QUIC Initial decryption (HTTP/3)

- **Decision**: Implement RFC 9001 §5 Initial protection with `ring` 0.17 (already in tree):
  - `hkdf` (HKDF-SHA256) derives keys: `HKDF-Extract(salt, DCID)` → `client in` secret → `quic key` / `quic iv` / `quic hp`. These are TLS 1.3 `HkdfLabel`s, which we encode ourselves.
  - `aead::quic::AES_128` removes header protection.
  - `aead::AES_128_GCM` decrypts the payload.
- **Versions**:
  - **v1**: version `0x00000001`, salt `38762cf7f55934b34d179ae6a4c80cadccbb7f0a`, labels `quic key`/`quic iv`/`quic hp`, Initial long-header type `0b00`.
  - **v2** (RFC 9369): version `0x6b3343cf`, salt `0dede3def700a6db819381be6e269dcbf9bd2ed9`, labels `quicv2 key`/`quicv2 iv`/`quicv2 hp`, Initial type `0b01`.
  - Correctness is pinned by the RFC 9001 Appendix A.2 and RFC 9369 Appendix A.2 client-Initial test vectors (unit tests).
  - Other versions are counted as `quic_unsupported_version`.
- **CRYPTO reassembly**: Parse decrypted frames: PADDING `0x00`, PING `0x01`, ACK `0x02`/`0x03` (skipped), CRYPTO `0x06` (offset varint, length varint, data), CONNECTION_CLOSE `0x1c`/`0x1d` (stop). Write CRYPTO data into a per-connection buffer by offset; it is complete when the handshake header (type 1, 24-bit length) is satisfied. Chrome's "chaos protection" splits, reorders and pads CRYPTO frames across 1–3 Initial packets, so offset-based reassembly across packets is required. Coalesced packets in one datagram are handled by walking the long-header lengths. Only client → server Initial packets are decrypted (FR-005). Keys are derived per DCID and cached for the connection.
- **Parse**: The reassembled handshake bytes form a bare `ClientHello` handshake message (no record layer), parsed with `tls-parser`'s handshake parser and shared with R4 extraction.
- **Alternatives**:
  - `quinn-proto`: endpoint-oriented, with no API for passively decrypting someone else's Initial packets.
  - RustCrypto `aes-gcm` + `hkdf` + `aes`: would add three crates when `ring` is already present.

## R6. DNS answers

- **Decision**: `hickory-proto` 0.26 (MIT/Apache-2.0) with default features off, only for message parsing. From each response (QR=1, RCODE NOERROR, UDP), build the chain `qname → CNAME… → A/AAAA addresses`. Bind each address to the **requesting device** (the response's destination IP) with expiry `clamp(ttl, 10 min, 1 h)`. Each binding keeps the queried name (the display name) and every alias in the chain (for matching, FR-011). DNS over TCP is out of scope (rare for A/AAAA).
- **Rationale**: Well-maintained, safe and widely deployed. Its parser is fuzzed upstream.
- **Alternatives**:
  - `simple-dns`: lighter, but a smaller user base.
  - `dns-parser`: unmaintained.

## R7. Plain HTTP Host

- **Decision**: `httparse` 1.10 (already in tree). Parse the first client → server payload on any TCP port when it starts with an HTTP method token, and take the `Host` header (port stripped, lowercased).
- **Rationale**: Trivial and already a dependency. Plain HTTP is rare but cheap to cover.

## R8. Matching captures to flow records

- **Decision**: Key = `(lan_ip, lan_port, remote_ip, remote_port, proto)` from the client → server packet. That equals the flow record's `(local_ip, local_port, remote_ip, remote_port, proto)` after feature 001's un-NAT (`LocalNetworks::to_record`). Lookup order at enrichment (FR-010):
  1. exact connection entry;
  2. device binding `(lan_ip, remote_ip)` from DNS;
  3. router DNS-cache name (existing).
- **Timing**: Setup packets arrive within milliseconds, and flow records 15–60 s later (inactive/active timeouts). Connection entries live 10 min after last lookup, refreshed whenever a flow matches. Late captures are covered by the existing 15-minute back-fill (DNS names feed `remote_hostname`, FR-012).
- **Direction caveat**: For inbound connections (port forwards) the LAN host is the server. Rule 1 captures client → server packets only on LAN ingress, so inbound connections are not named. That's acceptable and documented.

## R9. Where it lives (pluggability, FR-013)

- **Decision**: New crate `tmon-capture`, depending only on `tmon-core`:
  - TZSP + frame decode;
  - reassembly;
  - the TLS/QUIC/DNS/HTTP extractors;
  - the connection table and device bindings.

  `tmon-core` gains a small trait `ConnectionNamer` (`fn lookup(&self, key: &ConnKey) -> Option<ConnName>`). `tmon-enrich::Pipeline` holds an optional `Arc<dyn ConnectionNamer>`. A future nDPI sidecar would be another `ConnectionNamer` (or a feeder into the same table), with no change to collection, storage or UI.
- **Rationale**:
  - Keeps the hostile-input parsing surface in one crate with its own fuzz targets.
  - The collector crate stays flow-only.
  - The trait boundary is exactly what FR-013 needs.

## R10. Health detection (FR-019..FR-021)

- **Silent**: Capture is enabled and no TZSP datagram has arrived for 5 min → `capture_silent`.
- **Large-packet loss (WSL)**: Over a sliding 15-min window, `tls_started ≥ 50`, `tls_incomplete / tls_started ≥ 0.5`, and `max_datagram_len ≤ 1472` → `capture_large_packets_lost`, with remediation text. Rationale: full-size frames + TZSP header are about 1,519 bytes, so a healthy capture of modern ClientHellos always produces some datagrams over 1,472 bytes.
- **FastTrack bypass**: Over 15 min, `tcp_syn_seen ≥ 100` and `tcp_payload_first_seen / tcp_syn_seen < 0.1` → `capture_fastpath_suspected`. With FastTrack the SYN passes the normal path but later packets don't.
- Thresholds are constants (`tmon-capture/src/health.rs`) with unit tests.

## R11. Persisting the name source (FR-022)

- **Decision**: Add a nullable column `remote_host_source UTINYINT` to raw flows: `1` = TLS, `2` = QUIC, `3` = HTTP, `4` = DNS capture, `5` = router DNS cache, NULL = unknown. It is added to `flows_hot` by migration 6 (`ALTER TABLE … ADD COLUMN IF NOT EXISTS`).
- **Old Parquet files** lack the column. All three `read_parquet` call sites (`files.rs` union, `recategorize.rs` rewrite and rebuild) switch to `union_by_name = true`, which fills NULL. A regression test reads an old-schema file.
- The column is exposed in `/flows` rows and the CSV export as `remote_host_source` (`tls|quic|http|dns|router|null`), and shown as a small tag in the Flows view and host drill-down.
- **Not persisted**: the alias chain (used only for rule matching at ingest time). History re-apply sees only `remote_host`. That's documented as a limitation.

## R11a. Alias matching (FR-011)

- **Decision**: `FlowRecord` gains `remote_aliases: Option<Arc<[Arc<str>]>>` (not stored). The hostname matcher checks `remote_host`, then each alias. For DNS-derived names, `remote_host` is the queried name (what the user's device asked for, e.g. `www.youtube.com`) and the aliases are the CNAME targets (e.g. `youtube-ui.l.google.com`).

## R12. Concurrency and back-pressure (FR-015)

- **Decision**: One listener task per capture socket feeds a bounded channel (capacity 16,384 datagrams) to a single processing task that owns the connection table. When the channel is full the datagram is dropped and counted as `capture_dropped{reason="backpressure"}`. The flow pipeline only reads the table:
  - lookups go through an `Arc<ConnTable>` with sharded `Mutex<HashMap>` (16 shards);
  - flow ingest never blocks on capture processing;
  - lock hold time is a single map lookup.
- **Limits** (configurable, FR-014):
  - `max_connections` 100,000: LRU eviction per shard;
  - `max_reassembly_bytes_per_conn` 16 KB;
  - `max_reassembly_bytes_total` 64 MB: a global counter, with the oldest in-progress reassembly dropped when exceeded;
  - device bindings capped at 200,000.

## R13. Fuzzing and fixtures

- **Fuzz targets** (cargo-fuzz, like feature 001):
  - `capture_datagram`: the whole path, TZSP → extract, through one stateful processor so reassembly state is exercised;
  - `tls_client_hello`;
  - `quic_initial`;
  - `dns_response`.
  - Seeds come from the fixture generator.
- **Fixtures**: An `etherparse` builder in `tmon-capture/src/fixtures` generates TZSP pcaps:
  - a split ClientHello with a post-quantum-sized key share;
  - a QUIC v1 Initial built from the RFC 9001 A.2 vector;
  - a QUIC v1 Initial with CRYPTO split across 2 packets in reverse order, encrypted by the fixture generator with the same key schedule;
  - a QUIC v2 Initial from the RFC 9369 vector;
  - a DNS CNAME chain;
  - plain HTTP;
  - an ECH ClientHello;
  - FastTrack-like (SYN only) and WSL-like (truncated at 1,472 bytes) streams.

  The replay CLI (`tmon replay`) already sends pcap datagrams and is reused with `--to <capture addr>`.

## R14. Performance

- At home rates (≤ 200 new connections/s, ≤ 3 Mbit/s of capture), processing costs a few microseconds per datagram, with one HKDF + AES-GCM per QUIC connection.
- Memory at the default limits: about 100k connections × ~200 bytes ≈ 20 MB, plus reassembly ≤ 64 MB worst case. Typical use is far lower and stays under SC-004's 64 MB.
- A Criterion bench `capture_pipeline` (replaying the fixture mix) must show ≥ 50k datagrams/s on one core.
