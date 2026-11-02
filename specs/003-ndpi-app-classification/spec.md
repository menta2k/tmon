# Feature Specification: Application Classification with nDPI (Optional Sidecar)

**Feature Branch**: `003-ndpi-app-classification`

**Created**: 2026-11-01

**Status**: Draft

**Input**: User description: "nDPI application classification as an optional sidecar (feature 003, builds on 002 packet capture). Feature 002 names TLS/QUIC/HTTP/DNS connections in pure Rust, but traffic without a server name — P2P (BitTorrent), VPNs (WireGuard, OpenVPN, IPsec), games (Steam, Riot, Battle.net), VoIP/video calls (WhatsApp, Zoom, Teams STUN/RTP), remote desktop, and non-standard ports — is still categorized by port only ("guessed"). Add optional deep packet inspection using the nDPI library (LGPL-3.0, C) running in a separate sidecar process "tmon-dpi" so that a crash, hang or memory bug in C code can never take down tmon or lose flow records, and so the main tmon binary stays pure Rust without linking nDPI. tmon forwards the capture packets it already receives to the sidecar over a local bounded channel; the sidecar returns, per LAN-side 5-tuple, the detected application protocol, the nDPI category, confidence/detection method and any host name. tmon attaches the application to flow records during enrichment via the existing pluggable naming seam (002 names stay first). Rules gain an application match field ("detected" for confident detections, "inferred" for guesses); a built-in mapping from nDPI protocols/categories to tmon categories ships with the release. The application is stored per raw flow, shown in Flows, host drill-down and an Applications breakdown, exported in CSV and metrics. Optional and off by default. tmon supervises the sidecar (spawn, restart with backoff, heartbeat, protocol version check), reports its state on the Status page, and degrades gracefully. Bounded memory in both processes. Sidecar fuzzed at its IPC boundary, unprivileged, no network access, separate binary plus Docker image variant; LGPL obligations documented. Out of scope: risk scores and JA3/JA4 in the UI, inspection beyond the captured window, inline blocking, decryption."

## Context

Feature 002 names connections from the server name a client announces (TLS, QUIC, HTTP) and from DNS answers. Traffic that announces no name stays categorized by port only ("guessed") or not at all. Examples are peer-to-peer file sharing, VPN tunnels, online games, voice/video calls and remote desktop. On a home network that is a small share of connections but a large share of *questions* ("what is this device doing on UDP 51820?"). A mature open-source classifier (nDPI) recognizes several hundred such applications from the first packets of a connection. This feature adds it as an **optional, isolated helper process** that consumes the packets feature 002 already receives.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Recognize applications that announce no name (Priority: P1)

The user has packet capture (feature 002) running and turns on application classification. Connections such as a BitTorrent download, a WireGuard tunnel, a Steam game session or a WhatsApp call now show the application's name. They land in the right category (File Sharing, VPN, Gaming, Calls) with "detected" confidence instead of "guessed".

**Why this priority**: This is the value of the feature. Everything else (status, safety, packaging) only exists to deliver it safely.

**Independent Test**: With capture and classification enabled, replay (or generate) a capture containing a BitTorrent handshake, a WireGuard handshake, a STUN-based call and a Steam session, plus the matching flow records. Each flow shows its application and the expected category; the Applications breakdown shows those rules as "detected".

**Acceptance Scenarios**:

1. **Given** classification is enabled, **When** a LAN device starts a BitTorrent transfer, **Then** its flows show the application "BitTorrent" and are categorized as File Sharing with "detected" confidence.
2. **Given** a VPN tunnel on a non-standard port, **When** the tunnel starts, **Then** the flow shows the VPN application (e.g. WireGuard) and is categorized as VPN, although no port rule would match.
3. **Given** a connection already named by feature 002 (e.g. `www.netflix.com` from TLS), **When** the classifier also identifies it, **Then** the feature 002 name is kept, the application is added, and the category follows the existing rule priority.
4. **Given** the classifier only *guesses* an application (for example from the port), **When** the flow is categorized by that application, **Then** the confidence shown is "inferred", not "detected".
5. **Given** a user rule "application = Zoom → Work", **When** a Zoom call happens, **Then** the user rule wins over the built-in mapping.

---

### User Story 2 - See and use applications in the UI (Priority: P2)

The user sees the application next to each flow, filters and exports by it, sees per-device application usage in the host drill-down, and writes rules that match on it.

**Why this priority**: Story 1 already improves categories invisibly; this story makes the new information inspectable and usable.

**Independent Test**: After Story 1's replay, open Flows, filter by application "BitTorrent", export CSV, open the device's drill-down and create a rule on the application field. The filter, the export column, the drill-down list and the rule all work.

**Acceptance Scenarios**:

1. **Given** classified flows exist, **When** the user opens Flows, **Then** each classified flow shows its application, and the user can filter by it.
2. **Given** the user exports flows to CSV, **Then** the export contains an application column, empty for unclassified flows.
3. **Given** the user opens a device's drill-down, **Then** it lists the device's top applications by bytes.
4. **Given** the rule editor, **When** the user picks the application field, **Then** they can choose from known application names (with free-text fallback), test the rule against recent flows, and save it.
5. **Given** flows stored before this feature, **When** they are displayed, searched or re-categorized, **Then** they behave as "no application" without errors.

---

### User Story 3 - Know the classifier is healthy (Priority: P2)

The Status page shows whether the classifier is running, how much it classifies, and whether it drops work, restarts or crashes. Problems raise a warning banner with a fix.

**Why this priority**: An invisible helper process that silently stops working erodes trust. Users must be able to tell "nothing to classify" apart from "classifier broken".

**Independent Test**: Start tmon with classification enabled, kill the helper process, and overload it. The Status page shows the restart, the drop counters move, and a warning appears; it clears once the helper recovers.

**Acceptance Scenarios**:

1. **Given** classification is enabled, **When** the user opens Status, **Then** a classifier card shows state (running / restarting / failed / disabled), version, uptime, packets inspected, connections classified, top applications, queue drops and restart count.
2. **Given** the helper process crashes, **When** tmon notices, **Then** it restarts it with increasing back-off, counts the restart, and keeps collecting flows without loss.
3. **Given** the helper keeps crashing (e.g. 5 times in 10 minutes), **Then** tmon stops restarting it, marks it failed, and raises a warning explaining how to recover.
4. **Given** the helper is installed with an incompatible version, **Then** tmon refuses to use it and shows a clear warning naming both versions.
5. **Given** the helper is slower than the packet rate, **Then** excess packets are dropped and counted (never queued without bound) and a warning appears when drops exceed 5 % over 15 minutes.

---

### User Story 4 - Safe, isolated and optional (Priority: P3)

The classifier never endangers flow collection, the host or the user's privacy. It is off by default, runs as an unprivileged process without network access, holds bounded memory, keeps no payload, and its licensing is documented.

**Why this priority**: These are acceptance conditions for shipping native code that parses hostile input, not features users ask for directly.

**Independent Test**: Run with classification disabled and compare to feature 002 (identical behaviour). With it enabled, send malformed and flooding traffic: tmon keeps all flow records, both processes stay within memory limits, and the helper has no network access.

**Acceptance Scenarios**:

1. **Given** classification is not configured, **Then** no helper process is started and behaviour is identical to feature 002.
2. **Given** capture (feature 002) is disabled, **When** classification is enabled, **Then** tmon refuses the configuration with a message explaining that classification needs capture.
3. **Given** a flood of random or malformed packets, **Then** flow ingest loses no records, the helper stays within its memory limit, and parse failures are counted.
4. **Given** the helper process is running, **Then** it runs as an unprivileged user, cannot open network connections, and can only talk to tmon over the local channel.
5. **Given** the helper binary is missing or not executable, **Then** tmon starts normally without classification and shows a warning.

### Edge Cases

- A connection is classified only after its flow record was already enriched: the application is applied to later flow records of the same connection; already-stored records are back-filled within the same window as feature 002's late names (recent raw flows only).
- The classifier changes its answer mid-connection (e.g. a generic protocol refined to a specific application): the most specific, most confident answer wins; a guess never replaces a confident detection.
- Encrypted or unknown traffic the classifier cannot identify: the flow has no application and falls back to existing rules (no "Unknown" pseudo-application is stored).
- An application the built-in mapping does not know (new classifier release): the application is still stored and shown; the flow keeps its other categorization until a rule covers it.
- Two applications in one connection (e.g. TLS + a specific service): the more specific one is stored, the generic carrier (e.g. "TLS") is not.
- The helper restarts: per-connection state is lost; connections already in progress may stay unclassified; this is counted, not an error.
- Clock or ordering: packets for a connection arrive after the connection's flow records; same handling as feature 002 (bounded wait window, then back-fill).
- Very large application lists in filters and rule pickers stay usable (search-as-you-type).
- The helper produces a host name that disagrees with feature 002's name for the same connection: feature 002's name is kept.

## Requirements *(mandatory)*

### Functional Requirements

**Classification**

- **FR-001**: The system MUST support an optional application classifier that inspects the packets already received by the packet-capture input (feature 002) and identifies the application of each connection.
- **FR-002**: The classifier MUST run in a separate process from tmon. A crash, hang, or memory exhaustion of that process MUST NOT stop flow collection, lose flow records, or crash tmon.
- **FR-003**: For each identified connection, the classifier MUST provide the application name, the classifier's own category, whether the identification is confident or a guess, and any host name it observed.
- **FR-004**: tmon MUST attach the application to the matching flow records during enrichment, keyed by the same LAN-side connection identity as feature 002.
- **FR-005**: Names from feature 002 MUST keep priority. A classifier-provided host name MAY be used only when feature 002 has no name for the connection, and MUST be marked with its own name source.
- **FR-006**: A guess-based identification MUST NOT replace a confident one for the same connection, and generic carriers (e.g. plain TLS, QUIC, HTTP) MUST NOT be stored as the application when a specific application is known.

**Categorization and rules**

- **FR-007**: Rules MUST gain an application match field accepting one or more application names (case-insensitive).
- **FR-008**: Rule matches on a confident application MUST report "detected" confidence; matches on a guessed application MUST report "inferred".
- **FR-009**: The release MUST ship a built-in mapping from classifier applications and classifier categories to tmon categories (at least File Sharing/P2P, VPN, Gaming, Calls/Conferencing, Remote Access, Streaming, Social, and the existing categories). Built-in mappings MUST rank below user rules and below hostname rules for the same flow, and MUST be editable/disableable like other built-in rules.
- **FR-010**: Rule testing and "re-apply to history" MUST take the stored application into account.

**Storage, UI, export**

- **FR-011**: The application (and whether it was confident) MUST be stored per raw flow. Flows stored before this feature, including archived files, MUST read as "no application" without errors.
- **FR-012**: The Flows view MUST show the application and allow filtering by it; the CSV export MUST include it.
- **FR-013**: The host drill-down MUST show the device's top applications by bytes for the selected range.
- **FR-014**: An application breakdown (bytes and flows per application over a range) MUST be available through the API and shown in the UI.
- **FR-015**: Applications identified after a flow was stored MUST be back-filled into recent raw flows, using the same window as feature 002's late names.

**Supervision and health**

- **FR-016**: tmon MUST start the classifier process when enabled, check it is alive at a regular interval, and restart it with increasing back-off on failure. After 5 failures within 10 minutes it MUST stop restarting and report "failed" until the configuration is reloaded or tmon restarts.
- **FR-017**: tmon MUST verify the classifier's protocol version at start-up and refuse an incompatible classifier with a warning naming both versions.
- **FR-018**: Packets for the classifier MUST go through a bounded queue. When it is full, packets MUST be dropped and counted; the capture input and flow ingest MUST never wait on the classifier.
- **FR-019**: The Status page MUST show the classifier state, version, uptime, packets inspected, connections classified, top applications, queue drops, restarts and errors. The same counters MUST be exported as metrics.
- **FR-020**: Warnings MUST be raised for: classifier failed or not found, incompatible version, sustained drops (> 5 % of packets over 15 minutes), and repeated restarts. Each warning MUST include a remediation hint.

**Safety, privacy, packaging**

- **FR-021**: Classification MUST be off by default and require feature 002 capture to be enabled; enabling it without capture MUST be rejected at configuration time with a clear message.
- **FR-022**: Both processes MUST hold bounded memory: caps on tracked connections, per-connection classifier state, queue size, and idle timeouts; evictions MUST be counted.
- **FR-023**: The classifier process MUST run without elevated privileges, without network access, and with access only to the local channel to tmon. Where the host cannot enforce a restriction, tmon MUST log which restriction is missing.
- **FR-024**: No packet payload MUST be persisted or logged by either process; only the classification results leave the classifier.
- **FR-025**: The boundary between tmon and the classifier MUST tolerate malformed, truncated, oversized and out-of-order messages in both directions without crashing either side, and MUST be fuzz-tested.
- **FR-026**: The classifier MUST ship as a separate, optional binary and as a container image variant; the main tmon binary MUST NOT include the classifier library. The documentation MUST state the classifier library's license and what users and redistributors must do to comply.
- **FR-027**: The classifier MUST be replaceable: the channel and message format are documented so a different classifier could be used without changing tmon (continuing feature 002's FR-013).

### Key Entities

- **Application classification**: per connection (LAN device address/port, remote address/port, protocol): application name, classifier category, confident or guessed, optional host name, time of classification, expiry.
- **Application** (on a flow record): application name plus confidence flag; nullable; persisted.
- **Classifier process state**: running/restarting/failed/disabled/incompatible, version, start time, restart history, counters (packets sent, dropped, inspected, classified, errors, evictions), top applications.
- **Application mapping rule**: built-in rule that maps an application (or classifier category) to a tmon category, with priority below user and hostname rules.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On a typical home network with capture and classification enabled, the share of bytes categorized as "guessed" or uncategorized drops by at least half compared with feature 002 alone, measured over one day.
- **SC-002**: In a reference capture containing BitTorrent, WireGuard, OpenVPN, STUN/RTP calls, Steam and RDP, at least 90 % of those connections show the correct application and category.
- **SC-003**: With classification enabled and idle, flow ingest throughput stays within 5 % of the feature 002 baseline; under a capture flood, flow ingest loses 0 records.
- **SC-004**: Killing the classifier process 10 times during ingest causes 0 lost flow records, and classification resumes within 30 seconds of each restart.
- **SC-005**: The classifier process stays under 256 MB of memory at default limits under a sustained flood; tmon's own memory stays within feature 002's bound plus 16 MB.
- **SC-006**: A user can find which device used a given application in the last 24 hours in under 1 minute, using the Flows filter or the drill-down.
- **SC-007**: With classification disabled, all feature 002 acceptance tests pass unchanged.
- **SC-008**: Fuzzing the boundary between the processes for 10 minutes per message direction produces no crash.

## Assumptions

- Feature 002 packet capture is deployed and is the only packet source; the classifier sees only the first ~6 KB of each connection, which is enough for most handshake-based detections. Applications that need deeper inspection are out of scope.
- The classifier library is nDPI (latest stable 4.x/5.x at implementation time), used under its LGPL-3.0 license; tmon itself is not linked against it.
- The classifier runs on the same host as tmon (the local channel is not exposed over the network). Supported platforms for the classifier: Linux x86_64 and arm64 (native and container); on other platforms the feature is unavailable and tmon says so.
- The classifier's host names are lower-trust than feature 002's (heuristic extraction), hence their lower priority.
- Category names added by the built-in mapping (e.g. File Sharing, Remote Access, Calls) are created as built-in categories if missing; users can rename or recolor them.
- Default limits: up to 50k tracked connections in the classifier, 15-minute idle timeout, queue of 16k packets; adjustable in configuration.
- Out of scope: risk scores and JA3/JA4 fingerprints in the UI, inspection beyond the captured window, inline blocking/shaping, decrypting anything, running the classifier on a different host.
