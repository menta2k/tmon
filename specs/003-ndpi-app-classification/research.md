# Research: Application Classification with nDPI (feature 003)

All "NEEDS CLARIFICATION" items from the Technical Context are resolved here. Each section gives the **decision**, the **rationale** and the **alternatives** considered.

## R1. Classifier library and version

- **Decision**: nDPI **6.0** (released 2026-08-28, LGPL-3.0), pinned by tag. It is built from source with no optional dependencies: no libpcap, no json-c, nDPI's built-in gcrypt-light, no maxmind.
- **API used** (`ndpi_api.h` at tag 6.0):
  - `ndpi_init_detection_module` / `ndpi_finalize_initialization` / `ndpi_exit_detection_module`;
  - `ndpi_detection_process_packet(mod, flow, l3_packet, len, ts_ms, NULL)`;
  - `ndpi_detection_giveup`;
  - `ndpi_flow_free`, `ndpi_detection_get_sizeof_ndpi_flow_struct`;
  - `ndpi_get_proto_name`, `ndpi_category_get_name`, `ndpi_confidence_get_name`;
  - `ndpi_get_flow_name` (host);
  - `ndpi_set_memory_alloction_functions`, which lets us count and budget allocations (R9);
  - `ndpi_get_api_version` / `ndpi_revision`, used in the handshake (R7).
- **Rationale**: nDPI is the most complete open-source L7 classifier (≈ 450 protocols), is maintained, and is fuzzed upstream by OSS-Fuzz. It works on L3 packets, which feature 002 already decodes.
- **Alternatives**:
  - **libprotoident**: payload-size heuristics, smaller protocol set, less maintained.
  - **Our own signatures in Rust**: explicitly out of scope in feature 002, and a large ongoing maintenance burden.
  - **nDPI 5.0**: older. 6.0 is the current stable release.

## R2. Process model and language of the sidecar

- **Decision**: A separate binary `tmon-dpi`, written in **Rust**, links `libndpi.so` **dynamically** through a small internal `ndpi-sys` crate (bindgen at build time against the pinned headers, linked via `pkg-config`).
  - All `unsafe` code sits in one module, `ndpi.rs`, behind a safe `Classifier` API.
  - The sidecar lives in a **separate Cargo workspace** `backend/dpi/`, so the main workspace, CI and the main binary never need nDPI or a C toolchain.
- **Rationale**:
  - **Crash isolation** (FR-002): a segfault in C kills only the child.
  - **License hygiene** (FR-026): `tmon` is never linked against LGPL code.
  - **Shared code**: Rust lets the sidecar share the protocol codec with tmon (R3), plus our tooling: fuzzing, clippy, tests.
  - **Dynamic linking**: the simplest way to meet LGPL's relinking requirement.
- **Alternatives**:
  - **Sidecar in C**: no shared codec, a second toolchain and test stack.
  - **Linking nDPI into tmon**: violates FR-002 and FR-026.
  - **Wrapping `ndpiReader`**: it is a CLI tool for pcaps and has no streaming IPC.
  - **WASM-compiled nDPI**: immature, slow, and nDPI is not built for it.

## R3. Channel between tmon and the sidecar

- **Decision**: An `AF_UNIX` **`SOCK_SEQPACKET` socketpair** created by tmon before spawning the child. The child end is passed as **fd 3** using the `command-fds` crate. There is no filesystem path and no listening socket.
  - The format is a documented, versioned binary format ([contracts/dpi-ipc.md](contracts/dpi-ipc.md)), implemented in a pure-Rust crate `tmon-dpi-proto` used by both sides. Each message carries a 1-byte type and little-endian fixed fields, and is at most 9,400 bytes.
- **Rationale**:
  - SEQPACKET keeps message boundaries, so no framing bugs, and it is reliable and ordered.
  - A socketpair cannot be reached by other processes (FR-023).
  - A hand-specified format can be implemented by another classifier (FR-027) and is easy to fuzz (FR-025).
- **Alternatives**:
  - **A Unix stream socket with length prefixes**: framing errors are possible.
  - **A named socket path**: can be reached by other local users.
  - **Shared-memory ring**: unsafe, complex, and it does not isolate a hung peer.
  - **postcard/bincode**: tied to Rust/serde, so less suitable as a public contract.
  - **gRPC**: heavy, and it brings networking.

## R4. Which packets are forwarded, and when

- **Decision**: The capture processor (feature 002) gets a `PacketTap`. After a datagram decodes, the tap `try_send`s a **copy of the L3 packet** with its oriented ConnKey and a timestamp to a bounded queue (FR-018). The tap skips:
  - connections the sidecar has already reported as **final** (a `final` flag on Classification, kept in a small LRU "final set" in tmon);
  - packets beyond **`max_packets_per_conn`** (default 32, after which nDPI gives up anyway);
  - DNS answers (port 53): feature 002 already uses them, and nDPI gets the names from TLS/QUIC itself.
- **Orientation**: the LAN side is decided by `LocalNetworks` (feature 001) rather than "source = client". This allows the reverse direction to be captured too (R5).
- **Rationale**: re-using feature 002's socket, allow-list and decode means one input, one set of counters and one router configuration. It also bounds what nDPI sees to the captured window.
- **Alternatives**:
  - **The sidecar opens its own TZSP socket**: it would need network access (violates FR-023) and would duplicate the allow-list and decode.
  - **Forwarding raw TZSP datagrams**: the sidecar would re-parse untrusted framing in C-adjacent code.

## R5. Bidirectional capture

- **Decision**: Feature 002's rules capture LAN → WAN only. Many nDPI detections (STUN/RTP, BitTorrent, OpenVPN, games) improve markedly when the server's reply is seen.
  - The router guide gains an **optional** fourth mangle rule: `chain=forward in-interface-list=WAN connection-bytes=0-6000 action=sniff-tzsp`, with IPv6 too.
  - It is documented as roughly doubling the capture bandwidth, to about 2–6 Mbit/s.
  - Feature 002's processor ignores server → client segments for naming, which it already does via the ConnKey orientation. The tap forwards them.
- **Rationale**: this is a large accuracy gain at moderate cost, and it stays opt-in.
- **Alternative**: client → server only. Acceptable but weaker, and the reference capture (SC-002) is measured with both directions.

## R6. Attaching results to flows

- **Decision**: `tmon-core` gains `AppInfo { app: Arc<str>, category: Arc<str>, confident: bool, host: Option<Arc<str>>, final_: bool }` and a trait `ConnectionClassifier { fn classify(&self, key: &ConnKey, now_ms: i64) -> Option<Arc<AppInfo>> }`.
  - `Pipeline` holds an optional `Arc<dyn ConnectionClassifier>` next to the namer.
  - The implementation is an `AppTable`: sharded LRU, TTL = idle timeout, same shape as `ConnTable`, filled by the client task that reads Classification messages.
  - `FlowRecord` gains `app: Option<Arc<str>>` and `app_confident: bool`.
  - In enrichment, the app step runs **after** naming:
    1. If a 002 name exists, keep it.
    2. If not, and `AppInfo.host` is set, use the host with `NameSource::Dpi` (code 6).
    3. Set the app fields.
- **Merging updates** (FR-006): the newer result replaces the older only if `(confident, specificity)` is not lower. A generic carrier (TLS, QUIC, HTTP, DNS, or an unknown app) is never stored as the app. The sidecar sends the *app* protocol (`proto.app_protocol`) and falls back to the *master* protocol only when the master is not a generic carrier.
- **Confidence mapping** from `ndpi_confidence_t`:
  - **Confident** ("detected"): `DPI`, `DPI_CACHE`, `CUSTOM_RULE`.
  - **Guess** ("inferred"): `DPI_PARTIAL`, `DPI_PARTIAL_CACHE`, `MATCH_BY_IP`, `MATCH_BY_PORT`, `DPI_AGGRESSIVE`.
  - `UNKNOWN` is not reported at all.

## R7. Supervision

- **Decision**: A `Supervisor` task in tmon (new crate `tmon-dpi-client`).
  - **Spawn**: `tokio::process::Command` with `kill_on_drop`, the fd-3 socketpair, a clean environment, and the config passed as arguments.
  - **Handshake**: the child sends `Hello { proto_version, ndpi_version, ndpi_api, max_flows, app_catalog }` within 5 s.
    - tmon checks `proto_version == 1`. A mismatch puts it in state `incompatible` with a warning (FR-017).
  - **Liveness**: the child sends `Stats` every 2 s, which acts as a heartbeat. 3 missed heartbeats → kill (SIGKILL) and restart.
  - **Back-off**: 1 s, 2 s, 4 s … capped at 60 s. 5 failures within 10 min → `failed` (FR-016).
  - **Child side**: `prctl(PR_SET_PDEATHSIG, SIGKILL)`, and it exits when the socket closes.
- **States**: `disabled | starting | running | restarting | failed | incompatible | not_found`.
- **Rationale**: these are standard process-supervision semantics, and every failure mode maps to one visible state and one warning.
- **Alternatives**:
  - **systemd supervising the sidecar**: it would not work in Docker, needs a second unit, and tmon could not report state.
  - **No restart**: violates SC-004.

## R8. Sandbox (FR-023)

- **Decision**: The sidecar restricts itself after nDPI initialization, in this order. Each step is logged as applied or unavailable.
  1. `PR_SET_NO_NEW_PRIVS`.
  2. If running as root: `setgroups([])`, `setgid`/`setuid` to `nobody`. Normally tmon is already unprivileged (systemd `DynamicUser`, Docker non-root).
  3. **Landlock** (crate `landlock`, ABI ≥ 1): deny all filesystem access. nDPI needs no files after init.
  4. **seccomp** (crate `seccompiler`, the Firecracker project's pure-Rust compiler): an allow-list of `recvmsg`, `sendmsg`, `read`, `write`, `close`, `mmap`, `munmap`, `mremap`, `brk`, `madvise`, `futex`, `clock_gettime`, `exit_group`, `rt_sigreturn` and `sigaltstack`. Everything else (including `socket`, `connect`, `open*` and `execve`) kills the process. The result is no network and no file access.
  5. `setrlimit(RLIMIT_AS)` to `memory_limit_mb` + 64 MB as a hard backstop.
- **Rationale**: these are defense-in-depth layers that need no root and work inside Docker under the default seccomp profile. Landlock can be unavailable on older kernels, so it is best-effort and logged.
- **Alternatives**:
  - **bubblewrap / nsjail**: an extra runtime dependency, and unavailable in many containers.
  - **Docker-only network isolation**: the sidecar shares tmon's container network namespace, so this is not sufficient.

## R9. Memory bounds (FR-022, SC-005)

- **Decision**: nDPI allocations go through `ndpi_set_memory_alloction_functions` wrappers that count live bytes (each allocation records its size in a header).
  - **Flow table**: an LRU of `max_flows` (default 50k) `ndpi_flow_struct`s, about 1.1 KB each in 6.0 (measured in the plan's bench). Idle timeout is 15 min.
  - **Budget**: when live bytes exceed `memory_limit_mb` (default 192), the least recently used flows are given up and freed until the count is under 90 %. Evictions are counted.
  - `RLIMIT_AS` (R8) is the backstop.
  - **On the tmon side**: queue of 16k × ≤ 9.4 KB, but with a 16 MiB byte budget (the same pattern as feature 002); `AppTable` holds up to 100k entries of about 120 B.
- **Rationale**: SC-005 needs a hard ceiling. Counting at the allocator boundary is the only reliable way to account nDPI's internal growth (its LRU caches, per-flow buffers).
- **Alternative**: rely on `RLIMIT_AS` alone. It works, but the result is a crash and restart instead of graceful eviction.

## R10. Built-in application rules (FR-009)

- **Decision**: Add a `File Sharing` built-in category (id 15) and two rule match fields: `app` (names) and `app_category` (nDPI category names, for example `Download`, `VPN`, `Game`, `VoIP`, `RemoteAccess`). Matching is case-insensitive.
  - **New built-in rules** in `backend/assets/builtin-rules.toml`, in two priority bands:
    - `860_000–869_999`: named applications, for example BitTorrent → File Sharing, WireGuard/OpenVPN/IPsec/Tailscale → VPN/Tunnels, Steam/Riot/Battle.net/Xbox/PlayStation → Gaming, Zoom/Teams/WhatsApp call/STUN/RTP → Messaging/VoIP, RDP/AnyDesk/TeamViewer/VNC → Remote Access, Netflix/YouTube/Twitch → Streaming;
    - `850_000–859_999`: nDPI category fallbacks.
  - Higher priority wins. Both bands sit below the hostname rules (900k) and above the ASN/CIDR (800k) and port (700k) rules.
  - **Confidence**: `RuleMatch::confidence()` returns **Detected** when the rule has `app`/`app_category` and the flow's `app_confident` is true, and **Inferred** otherwise. Confidence becomes a function of (rule, flow), with an extra argument at the one call site.
- **Rationale**:
  - Data-driven, like the existing built-ins.
  - User rules (priority ≥ 1,000,000) still win.
  - Hostname evidence (002) stays stronger than app evidence for the same flow.
- **Alternative**: let nDPI's own category decide the tmon category directly. Rejected because users could not override it, and it would bypass the rule engine and "re-apply to history".

## R11. Storage

- **Decision**:
  - **Migration 7** adds `app VARCHAR` and `app_confident BOOLEAN` to `flows_hot` (and `batch_stage`). Old Parquet files are read as NULL through `parquet_rows()` (`union_by_name`), as established in 002.
  - **`remote_host_source`** gains code `6 = dpi`.
  - **Application breakdown** (`/traffic/breakdown?by=app`) and `/hosts/{ip}` top apps are served from **raw flows only**. Ranges beyond raw retention return the existing `unsupported_filter_range` error. No new rollup in v1.
  - **Back-fill**: the client queues late classifications that arrive after the flow was stored. Every 5 s, `backfill_apps` updates `app`/`app_confident` on raw `flows_hot` rows of the last 15 min with matching 5-tuple and NULL app, then re-categorizes them, reusing 002's back-fill pattern.
- **Rationale**: there are only 2 nullable columns, and a raw-only breakdown avoids a rollup-cardinality explosion (apps × hosts × hours). A rollup can be added later if users need 30-day app history.
- **Alternatives**:
  - **Store numeric nDPI ids**: they change between nDPI versions.
  - **New `rollup_app_1h`**: deferred.

## R12. Fuzzing and testing without nDPI

- **Decision**:
  - **Main workspace**: fuzz targets `dpi_proto_to_sidecar` and `dpi_proto_to_tmon` exercise the codec decode in both directions (SC-008). A **fake sidecar** (`tmon-dpi-client/examples/fake_dpi.rs`, pure Rust) speaks the protocol with scripted behaviour (classify by port, crash after N, hang, wrong version, slow, oversize). It drives all supervisor, back-pressure and integration tests in CI without nDPI.
  - **`backend/dpi/` workspace**:
    - a fuzz target `sidecar_packets` (codec → nDPI flow table) runs under libFuzzer with ASan against a static debug build of nDPI;
    - integration tests replay generated fixture packets for BitTorrent, WireGuard, OpenVPN, STUN/RTP, Steam A2S and RDP through the real classifier (SC-002);
    - a separate CI job builds nDPI 6.0 (cached).
- **Fixture generators**: these are added to `tmon-capture/src/fixtures/apps.rs` from public protocol specs:
  - BitTorrent BEP 3 handshake;
  - WireGuard handshake initiation/response (type 1/2, 148/92 bytes);
  - OpenVPN `P_CONTROL_HARD_RESET_CLIENT_V2`;
  - STUN binding request/response (RFC 8489 magic cookie) followed by RTP;
  - Steam A2S_INFO query/response;
  - RDP X.224 Connection Request with `Cookie: mstshash=`;
  - a TLS hello to `zoom.us` (to check precedence).
- **Rationale**: most logic is testable in the default CI without C. The C boundary gets its own, smaller, nDPI-aware suite.

## R13. Packaging and licensing (FR-026)

- **Decision**:
  - **`deploy/Dockerfile`** gains a target `tmon-dpi`. The build stage compiles nDPI 6.0 (`./autogen.sh && ./configure --with-only-libndpi && make`), then builds `backend/dpi`. The runtime image = the base tmon image + `/usr/local/bin/tmon-dpi` + `/usr/local/lib/libndpi.so.6` + `/usr/share/doc/tmon-dpi/` (nDPI `COPYING`, a written source offer, the nDPI tag and a source URL).
  - **Image tags**: `tmon:latest` (no nDPI) and `tmon:latest-dpi`.
  - **Release tarballs**: `tmon-dpi` is shipped as a separate archive with `libndpi.so` and the license files.
  - **Docs**: `docs/dpi.md` covers what, why and how to enable it, the LGPL obligations (provide nDPI source or a written offer, keep the license, allow relinking, which dynamic linking satisfies, and do not static-link in redistributed builds), and tmon's MIT license for its own code.
- **Rationale**: users who never enable DPI never download C code. Redistributors get a clear checklist.
- **Alternative**: static linking in the image. That is possible under LGPL with object files for relinking, but it is needlessly complex.

## R14. Performance targets

- **Decision**:
  - **Sidecar**: ≥ 20k packets/s on one core with the fixture mix (nDPI is typically 1–5 µs per packet). The home load at 32 packets/conn × 200 conn/s is ≈ 6.4k packets/s.
  - **tmon side**: the tap adds < 1 µs per datagram (`try_send` of a copy). The capture bench must stay ≥ 50k datagrams/s with DPI enabled.
  - **Flow ingest**: within 5 % (SC-003), checked with `bench-ingest.sh` (`DPI=1`).
