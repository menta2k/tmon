# Feature Specification: Connection Naming from Router Packet Capture

**Feature Branch**: `002-connection-naming`

**Created**: 2026-11-01

**Status**: Draft

**Input**: User description: "Connection naming from router packet capture (Option A, pure Rust, no nDPI). Today tmon learns remote host names only by polling the MikroTik DNS cache every 30 s, so only ~20% of flows get a hostname and ~97% of home traffic is categorized by port only ("guessed"). Add a second input: the router streams the first few KB of every connection to tmon (RouterOS firewall mangle action=sniff-tzsp with connection-bytes=0-6000, TZSP over UDP, default port 37008). tmon decodes TZSP → Ethernet → IPv4/IPv6 → TCP/UDP and extracts, per connection: the TLS ClientHello server name (SNI) and ALPN (with reassembly of ClientHellos split over 2+ TCP segments), the QUIC Initial ClientHello SNI/ALPN (QUIC v1 and v2, CRYPTO frames split over several Initial packets and reordered), DNS responses (A/AAAA/CNAME answers per requesting LAN device, including the CNAME chain), and the plain-HTTP Host header. Results are kept in a short-lived connection table keyed by the LAN-side 5-tuple and attached to flow records during enrichment, so existing hostname rules match with "detected" confidence; DNS-learned names also feed the existing remote-hostname table. Optional, off unless a capture listener is configured; DNS-cache polling remains as fallback. Memory-safe, fuzzed, bounded in memory, drops and parse errors reported on the Status page. FastTrack setup guidance; WSL2 mirrored-mode large-datagram loss detected and reported. Out of scope: nDPI, JA3/JA4, non-TLS payload signatures, decrypting anything beyond QUIC Initial packets. The naming step must be pluggable so a deeper classifier could be added later as an optional separate process."

## Context

Feature 001 (tmon) categorizes traffic from flow records. Flow records carry addresses and ports but no names, so tmon learns remote host names by periodically reading the router's DNS cache. On a real home network this names only about 20% of flows, and about 97% of traffic ends up categorized by port alone (shown as "guessed" confidence). Most home traffic is encrypted web traffic whose connection setup still announces the destination name in clear text. This feature lets tmon read those names from the first packets of each connection, which the router can forward to it.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Encrypted web and video traffic is identified by name (Priority: P1)

The network owner enables connection capture on the router (one firewall rule, following the setup guide) and turns on the capture input in tmon. From then on, new connections to websites, video services, app stores, AI services and similar are shown with their real destination name and land in the right category with "detected" confidence, instead of "Web (guessed)".

**Why this priority**: This is the whole point of the feature: turning "97% guessed" into mostly "detected" for the traffic that dominates home networks (secure web and HTTP/3).

**Independent Test**: With capture enabled, open a video in a browser and a few websites from a LAN device (or replay a capture containing them). Verify the flows show the destination name, the video lands in Streaming, and the Applications view shows the matching rules as "detected".

**Acceptance Scenarios**:

1. **Given** capture is enabled and the router forwards the first packets of each connection, **When** a LAN device opens a secure web connection to a named site, **Then** the resulting flow shows that site name as its remote host and is categorized by the matching hostname rule.
2. **Given** the browser uses HTTP/3 (over UDP), **When** it connects to a named site, **Then** the flow is named the same way as for HTTP/1.1 or HTTP/2.
3. **Given** a modern browser whose connection-setup message is too large for one packet, **When** it connects, **Then** the site name is still recovered from the combined packets.
4. **Given** the same remote address serves different sites to different devices (shared CDN), **When** two devices connect to it for different sites, **Then** each device's flow carries the name that device actually asked for.

---

### User Story 2 - Names from DNS answers, per device (Priority: P2)

tmon also reads the DNS answers the router forwards. Each name a device looked up is linked to the addresses it resolved to, for that device, including aliases (CNAME chains). Traffic that does not announce a name at connection setup (non-web protocols, or connections the capture missed) still gets named. The learned names also improve the existing remote-host lists and the history back-fill.

**Why this priority**: It covers what Story 1 misses (non-TLS traffic, missed handshakes) and makes naming immediate instead of waiting up to 30 s for the next DNS-cache poll. It is useful but secondary to Story 1, which covers most bytes.

**Independent Test**: With capture enabled, resolve a name from a LAN device and then connect to it with a non-web protocol. Verify the flow gets the looked-up name, and that an alias chain resolves to the final name with the original queried name also available.

**Acceptance Scenarios**:

1. **Given** a device resolves `name A`, which is an alias of `name B`, which resolves to address X, **When** that device then sends traffic to X, **Then** the flow is named and categorized using both names (a rule matching either A or B applies).
2. **Given** two devices resolved different names to the same address, **When** each sends traffic there, **Then** each flow uses the name its own device looked up.
3. **Given** capture is enabled, **When** a DNS answer is seen, **Then** the name is available to new flows within seconds, without waiting for a router DNS-cache poll.

---

### User Story 3 - Know whether capture is working, and how to set it up (Priority: P2)

The owner can see on the Status page whether capture packets are arriving, how many connections were named, and any problems: no packets, packets being dropped for capacity, unparseable packets, or signs that large packets are lost on the way. The setup guide explains the router rule, the FastTrack interaction and the host networking limitation, with concrete fixes.

**Why this priority**: Capture depends on router configuration that is easy to get subtly wrong. Without visible health signals, a misconfiguration looks like "the feature does nothing".

**Independent Test**: Enable the capture input without configuring the router: Status shows "no capture packets received". Send capture packets: counters move. Send only small fragments of large handshakes: a warning about lost large packets appears with remediation text.

**Acceptance Scenarios**:

1. **Given** the capture input is enabled but nothing arrives for 5 minutes, **When** the user opens Status, **Then** a warning says capture is enabled but silent and links to the setup guide.
2. **Given** capture packets arrive, **When** the user opens Status, **Then** they see packets received, connections tracked, connections named (by source: secure web, HTTP/3, DNS, plain web), parse errors and capacity drops.
3. **Given** connection-setup messages repeatedly arrive incomplete in a way consistent with large packets being dropped in transit, **When** the user opens Status, **Then** a warning explains that large capture packets are being lost and what to do (run tmon natively or in a container, or change how the host networks).
4. **Given** the router fast-paths established connections so that connection-setup messages are never captured, **When** capture runs, **Then** Status shows that connections are seen but rarely named, and the guide explains how to exempt the first bytes of each connection from fast-path.

---

### User Story 4 - Capture stays safe, bounded and optional (Priority: P3)

The capture input is off unless configured. It accepts packets only from allowed router addresses, never stores packet contents, and keeps working within fixed memory limits even under floods or malformed input. When it is off or broken, everything else works as before.

**Why this priority**: These are guardrails rather than new user value, but the feature is unacceptable without them.

**Independent Test**: Run with capture disabled and confirm behaviour identical to before. Flood the capture port with random and malformed data: memory stays under the configured cap, counters show errors and drops, and flow ingest continues unaffected.

**Acceptance Scenarios**:

1. **Given** no capture input is configured, **When** tmon runs, **Then** behaviour, resource use and stored data are unchanged from before this feature.
2. **Given** a packet arrives from an address not on the allowed list, **When** it is received, **Then** it is discarded and counted.
3. **Given** a flood of connections or malformed packets, **When** the tracked-connection or buffer limits are reached, **Then** the oldest state is evicted, the event is counted, memory stays bounded, and flow ingest is not slowed.
4. **Given** any capture packet, **When** it is processed, **Then** only extracted names, the protocol hint and the connection identity are kept, never payload bytes.

---

### Edge Cases

- **Encrypted Client Hello**: the visible name is a generic front name (for example a CDN's ECH front domain). It is recorded as such and is not treated as the real site. The connection falls back to DNS-learned or router-cache names when available.
- **DNS over HTTPS/TLS** used by a device: DNS answers are not visible; naming relies on connection-setup names.
- **Connection seen only after its setup** (capture started mid-connection, or setup packets lost): the connection gets no setup name and falls back to DNS or router-cache names.
- **Flow record arrives before capture data** (late or reordered capture packets): late names are still applied through the existing short-term back-fill of recent flows.
- **NAT**: capture is taken on the LAN side, so connections are identified by the LAN device address. Flow records are matched using the LAN-side identity established by feature 001's NAT handling.
- **Duplicate capture** (the same packet captured on two interfaces): idempotent, with no double counting. Capture never contributes to byte counts.
- **Out-of-order and retransmitted TCP segments** in the first bytes: reassembly tolerates gaps up to the per-connection buffer limit, then gives up on that connection.
- **QUIC versions** other than v1/v2, or Initial packets that cannot be decrypted: counted as unparsed, with no name.
- **Very long names, malformed name fields, or non-ASCII names**: rejected or normalized safely and counted. They never break categorization.
- **IPv6, VLAN-tagged frames**: supported. Other encapsulations are counted as unsupported.
- **Router sends truncated packets**: names are extracted when the needed bytes are present. Otherwise the packet is counted as incomplete.
- **Clock and timing**: connection state expires after a bounded time even if no flow record ever matches it.

## Requirements *(mandatory)*

### Functional Requirements

**Input and decoding**

- **FR-001**: System MUST provide an optional capture input that receives packets forwarded by the router in the router's packet-streaming format (TZSP) over UDP, on a configurable listen address (default port 37008). It MUST be disabled unless configured.
- **FR-002**: System MUST accept capture packets only from allowed sources (reusing the existing allowed-exporter list when set) and count rejected packets.
- **FR-003**: System MUST decode Ethernet frames (with or without VLAN tags) carrying IPv4 or IPv6 and TCP or UDP, and count anything else as unsupported.

**Name extraction**

- **FR-004**: System MUST extract the requested server name and the application protocol list from TLS connection-setup messages, including messages that span multiple TCP segments within the first bytes of a connection.
- **FR-005**: System MUST extract the same information from HTTP/3 (QUIC version 1 and version 2) connection-setup packets, including setup data split across several packets or delivered out of order. No other QUIC packets are decrypted.
- **FR-006**: System MUST extract name → address mappings from DNS answers (A, AAAA and CNAME, including alias chains), associated with the LAN device that asked.
- **FR-007**: System MUST extract the requested host from plain (unencrypted) HTTP requests.
- **FR-008**: System MUST recognize a connection-setup name that is a known encrypted-client-hello front name and mark it as such rather than as the destination.

**Correlation and categorization**

- **FR-009**: System MUST keep a short-lived table of named connections keyed by the LAN-side connection identity (LAN address and port, remote address and port, protocol). Entries MUST expire after a bounded time (default 10 minutes after last use).
- **FR-010**: During flow enrichment, System MUST attach the best available remote host name to each flow, in this order:
  1. the connection-setup name for that exact connection;
  2. a DNS-learned name for that device and remote address;
  3. the existing router DNS-cache name.
- **FR-011**: Names obtained under FR-010 MUST be visible to categorization rules exactly like existing remote host names, so existing hostname rules apply and report "detected" confidence. When an alias chain is known, a rule matching any name in the chain MUST apply.
- **FR-012**: DNS-learned names MUST also update the shared remote-host name list and trigger the existing recent-flow back-fill, so flows that arrived before the name are corrected.
- **FR-013**: The naming step MUST sit behind a replaceable interface, so a different or additional classifier (for example one running as a separate process) can be added later without changing collection, storage or the user interface.

**Limits, safety and privacy**

- **FR-014**: System MUST bound memory with configurable limits on:
  - tracked connections (default 100,000);
  - reassembly bytes per connection (default 16 KB);
  - total reassembly bytes (default 64 MB).

  It MUST evict the oldest state and count evictions when a limit is reached.
- **FR-015**: Capture processing MUST NOT delay or drop flow-record ingest. If capture cannot keep up, capture packets are dropped and counted instead.
- **FR-016**: System MUST NOT persist packet payloads. Only extracted names, the protocol hint (e.g. "h2", "h3") and the source of the name are kept, and only in memory except where names feed the existing remote-host list.
- **FR-017**: All capture parsing MUST be robust against arbitrary input: no crash, hang or unbounded growth on malformed, truncated or adversarial packets. This MUST be verified with automated fuzzing, like the existing flow decoders.

**Visibility**

- **FR-018**: The Status page MUST show, for the capture input:
  - whether it is enabled and listening;
  - packets received, rejected, unsupported and unparsed;
  - connections tracked and evicted;
  - names extracted, by source (secure web, HTTP/3, DNS, plain web);
  - share of recent flows that received a name.
- **FR-019**: System MUST raise a Status warning when capture is enabled but no packets arrived for 5 minutes.
- **FR-020**: System MUST raise a Status warning when capture data shows signs that large packets are lost in transit: connection-setup messages repeatedly start but never complete, while no capture datagram larger than the size that survives without fragmentation is ever received. The warning MUST include remediation guidance.
- **FR-021**: System MUST raise a Status warning when many connections are observed but almost none show their setup messages (consistent with router fast-path bypass), with guidance.
- **FR-022**: The flow detail views (raw flow search and host drill-down) MUST show where a remote host name came from (connection setup, DNS, router cache) for flows named under this feature.

**Documentation**

- **FR-023**: The router setup guide MUST cover:
  - the capture rule (first ~6 KB of each connection only);
  - how to keep the router's fast-path from bypassing it;
  - the expected bandwidth cost;
  - how to verify capture is working;
  - the host-networking limitation on large UDP datagrams (WSL2 mirrored mode), with the recommended alternatives (run natively or in a container).

### Key Entities

- **Capture source**: a router allowed to send capture packets. Has counters for received, rejected and unsupported packets, and the largest datagram seen.
- **Tracked connection**: a LAN-side connection identity plus partial setup data being reassembled (bounded) and, once known, its name, protocol hint and name source. It expires after inactivity.
- **Device name binding**: a LAN device, a remote address and the name(s) that device resolved to that address (including the alias chain), with an expiry. Derived from DNS answers.
- **Name source**: where a flow's remote host name came from: connection setup (secure web / HTTP/3 / plain web), DNS answer, or router DNS cache.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On a typical home network with capture correctly set up, at least 80% of bytes on secure web ports (TCP/UDP 443) are attributed to a named destination within 1 hour of enabling it (today about 20% of flows are named).
- **SC-002**: The share of traffic categorized with "guessed" confidence or not at all drops below 40% of bytes, against about 97% today, using only the existing built-in rules.
- **SC-003**: For a connection whose setup packets were captured, its flow shows the name the first time it appears in tmon, with no waiting for a later poll or back-fill.
- **SC-004**: With capture enabled at home-network rates (up to 200 new connections per second), tmon stays within the existing resource targets, with less than 64 MB additional memory, and flow ingest throughput is unchanged.
- **SC-005**: Under a flood of random or malformed capture packets for 10 minutes, tmon keeps running, memory stays within the configured limits, and no flow records are lost.
- **SC-006**: A user following the setup guide can enable capture and see names appearing within 15 minutes. When something is misconfigured (no packets, fast-path bypass, large-packet loss), the Status page names the problem and the fix.
- **SC-007**: With capture disabled, all existing feature 001 tests and benchmarks pass unchanged.

## Assumptions

- The router is a MikroTik running RouterOS v7 with firewall packet-streaming (TZSP) support. Other packet sources (switch mirror ports, other capture tools) can use the same input if they emit the same format, but are not separately supported.
- Capture is taken on the LAN side of the router (bridge or LAN interface), so LAN device addresses are visible. Capturing on the WAN side is out of scope.
- Only the first ~6 KB of each connection is forwarded. That is enough for connection-setup messages and typical DNS answers. Larger DNS answers may be partially parsed.
- Encrypted Client Hello and DNS over HTTPS/TLS will hide some names. This is accepted: those connections fall back to other name sources or stay unnamed.
- The router's packet-streaming rule has no packet truncation option, so full-size frames are forwarded. In environments that drop large UDP datagrams (WSL2 mirrored networking), tmon must run natively or in a container. The feature detects and reports the problem but cannot work around it.
- Feature 001's NAT handling (LAN-side identity for downloads) is in place and is the key used to match captured connections to flow records.
- Captured data is processed on the same machine as tmon and never leaves it. Names are retained only as described in FR-016, under the existing retention settings.
- Out of scope: deep payload inspection libraries (nDPI), client fingerprints (JA3/JA4), signatures for non-TLS protocols (P2P, VPN handshakes, games), and decryption of anything beyond HTTP/3 connection-setup packets.
