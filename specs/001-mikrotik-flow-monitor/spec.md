# Feature Specification: MikroTik Flow Monitor (tmon)

**Feature Branch**: `001-mikrotik-flow-monitor`

**Created**: 2026-10-31

**Status**: Draft

**Input**: User description: "Collect sFlow / NetFlow v9 / IPFIX flow telemetry exported by a MikroTik router (RouterOS Traffic Flow) and categorize the traffic (by application/service, protocol, host/device, direction, destination ASN/country). Single-user (no multi-user/RBAC), self-hosted, lightweight. Backend in Rust, frontend in Vue. Storage: evaluate VictoriaMetrics for fit; if it does not fit, recommend an alternative."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See live traffic broken down by category (Priority: P1)

The network owner points their MikroTik router's flow export at tmon and opens the web dashboard. Within a minute they see current and recent bandwidth broken down by category (e.g. Streaming, Gaming, Web, Messaging, Updates, VPN, DNS, Other), by protocol, and by direction (download / upload / internal).

**Why this priority**: This is the core value: "what is my network doing right now and what kind of traffic is it". Without it nothing else matters.

**Independent Test**: Configure a router (or a flow replay tool) to export flows to tmon, generate known traffic (e.g. a video stream, a large download), and verify the dashboard shows the traffic under the expected category and direction with plausible volumes.

**Acceptance Scenarios**:

1. **Given** tmon is running and the router exports NetFlow v9 or IPFIX to it, **When** the user opens the dashboard, **Then** they see total throughput and a per-category breakdown for the last 15 minutes, refreshed at least every 10 seconds.
2. **Given** flows arrive for traffic on a well-known service port (e.g. 443/UDP to a video provider), **When** the flow is processed, **Then** it is assigned to the matching category; unmatched traffic is assigned to "Other".
3. **Given** the user selects a time range (last 1h, 24h, 7d, 30d or custom), **When** the range is applied, **Then** all charts and tables reflect only that range.

---

### User Story 2 - Find out who is using the bandwidth (Priority: P1)

The user wants to know which local device is consuming bandwidth and with whom it is talking. They open a "Top talkers" view listing local hosts ranked by volume, then drill into one host to see its categories, top remote destinations (with country and network owner/ASN), and protocols over time.

**Why this priority**: The most common reason to run a flow monitor is answering "who is eating the bandwidth". Equal in importance to Story 1.

**Independent Test**: Generate traffic from two distinct LAN hosts with different volumes and verify the ranking, per-host drill-down and remote-destination enrichment are correct.

**Acceptance Scenarios**:

1. **Given** flows from several LAN hosts, **When** the user opens Top Talkers for a time range, **Then** hosts are ranked by total bytes with separate upload/download figures.
2. **Given** a host is selected, **When** the drill-down opens, **Then** it shows that host's traffic over time, per-category split, and the top 20 remote endpoints with country, ASN and network owner name.
3. **Given** the remote endpoint is a public address, **When** it is displayed, **Then** country and ASN are shown; for private/reserved addresses they are labelled "Local/Private".

---

### User Story 3 - Customize categories and name devices (Priority: P2)

The user edits categorization rules (e.g. "anything to ASN 2906 is Streaming", "UDP 27015–27030 is Gaming", "host 192.168.88.50 is the NAS") and gives friendly names to local devices. Changes apply to new traffic immediately and the user can optionally re-categorize historical raw data still within retention.

**Why this priority**: Built-in categories cover common cases, but every network has its own devices and services. Important, but the product is useful without it.

**Independent Test**: Add a rule matching a specific port, generate traffic on that port, verify it appears under the new category; rename a device and verify the name is shown everywhere that host appears.

**Acceptance Scenarios**:

1. **Given** a new rule matching by any combination of protocol, port/port range, remote IP/CIDR, ASN, country, local host or hostname pattern, **When** the user saves it, **Then** flows received afterwards matching the rule get the rule's category.
2. **Given** multiple rules match a flow, **When** it is categorized, **Then** the rule with the highest user-defined priority wins, and user rules always override built-in rules.
3. **Given** the user assigns a friendly name to a local IP or MAC address, **When** any view shows that host, **Then** the friendly name is displayed alongside the address.
4. **Given** a rule set change, **When** the user requests "re-apply to history", **Then** raw flow data still within retention is re-categorized and summaries are rebuilt.

---

### User Story 4 - Enrich using router knowledge (Priority: P3)

The user optionally gives tmon read-only access to the router's management interface. tmon then learns local device names from the router's DHCP leases and remote hostnames from the router's DNS cache, so hosts appear as "Johns-iPhone" and remote endpoints as "*.googlevideo.com", which also enables hostname-based categorization rules.

**Why this priority**: Large improvement in readability and categorization accuracy, but flow data alone is sufficient for a working product.

**Independent Test**: Enable router integration, connect a device that gets a DHCP lease and resolves a domain, verify both names appear in the UI and that a hostname-pattern rule matches its traffic.

**Acceptance Scenarios**:

1. **Given** router integration is enabled with valid read-only credentials, **When** a device holds a DHCP lease with a hostname, **Then** that hostname is shown for the device (unless the user set a friendly name, which wins).
2. **Given** a remote IP appears in the router's DNS cache, **When** traffic to that IP is shown, **Then** the cached hostname is shown and can be matched by hostname rules.
3. **Given** the router becomes unreachable, **When** integration polling fails, **Then** flow collection continues unaffected and the UI shows an integration warning.

---

### User Story 5 - Know that collection is healthy (Priority: P2)

The user can see whether flows are arriving, from which exporters, at what rate, and whether anything is being dropped or misparsed, so they can trust the numbers.

**Why this priority**: Flow monitoring silently fails in many ways (wrong port, missing templates, sampling). Visible health is required to trust the data.

**Independent Test**: Stop the exporter and verify a "no flows received" warning appears; send malformed packets and verify they are counted as errors without stopping collection.

**Acceptance Scenarios**:

1. **Given** flows are being received, **When** the user opens the Status page, **Then** they see each exporter's address, protocol/version, flows per second, last-seen time, sampling rate and error counters.
2. **Given** no flows have arrived for 5 minutes, **When** the user opens any page, **Then** a visible warning indicates collection has stalled.
3. **Given** NetFlow v9/IPFIX data arrives before its template, **When** it is processed, **Then** it is buffered briefly or counted as "awaiting template" and never causes a crash.

---

### User Story 6 - Explore and export raw flows (Priority: P3)

For troubleshooting, the user searches recent raw flow records filtered by any dimension (host, remote IP, port, protocol, category, country, ASN) and exports the result as CSV.

**Why this priority**: Power-user and troubleshooting feature; summaries answer most questions.

**Independent Test**: Filter raw flows by a known host and port, verify results match generated traffic, export to CSV and check the content.

**Acceptance Scenarios**:

1. **Given** raw flows within retention, **When** the user applies filters and a time range, **Then** matching flows are listed newest first with paging.
2. **Given** a result set, **When** the user exports, **Then** a CSV file with all listed columns is downloaded (capped at a documented maximum row count).

---

### Edge Cases

- Exporter uses sampling (e.g. 1-in-N): volumes MUST be scaled by the sampling rate and the UI MUST indicate that figures are estimates.
- Router restarts or changes templates: new templates replace old ones without restarting tmon.
- Multiple exporters (or several interfaces on one router) report the same traffic: the user can choose which exporter/interface direction counts, to avoid double counting.
- Long-lived flows spanning many minutes: volume is attributed to the time interval in which it was reported (active timeout), not all to the flow's start.
- IPv6 traffic: handled identically to IPv4, including enrichment and categorization.
- Traffic NAT'ed by the router: the local (pre-NAT) host is used for attribution when the export contains it; otherwise the limitation is shown in Status.
- Disk filling up: oldest raw data is removed first; the service keeps running and warns the user.
- Bursts beyond configured capacity: excess packets are dropped and counted, never crashing the service.
- Enrichment databases (country/ASN) missing or outdated: flows are still stored; enrichment fields show "Unknown" and Status shows a warning.
- Clock skew between router and tmon: tmon uses its own receive time when exporter timestamps are implausible (more than 5 minutes off).

## Requirements *(mandatory)*

### Functional Requirements

**Collection**

- **FR-001**: System MUST receive flow export over UDP in NetFlow v5, NetFlow v9 and IPFIX formats as produced by MikroTik RouterOS Traffic Flow.
- **FR-002**: System MUST receive sFlow v5 (flow samples and counter samples) as produced by MikroTik switch-chip sFlow.
- **FR-003**: System MUST manage NetFlow v9/IPFIX templates per exporter and observation domain, including template refresh and replacement.
- **FR-004**: System MUST apply exporter-declared or user-configured sampling rates to byte and packet counts.
- **FR-005**: System MUST let the user configure listening ports and an allow-list of exporter addresses; packets from other sources are dropped and counted.
- **FR-006**: System MUST sustain the target ingest rate (see SC-002) without data loss and MUST count, not crash on, malformed or excess packets.

**Classification & enrichment**

- **FR-007**: System MUST classify each flow's direction as download, upload, internal (local↔local) or transit, based on user-configurable local networks (defaulting to RFC1918 and IPv6 ULA ranges).
- **FR-008**: System MUST enrich public remote addresses with country, ASN and AS organisation name using locally stored, offline databases that the user can update.
- **FR-009**: System MUST assign every flow exactly one category from an ordered rule set; a built-in rule set MUST cover at least: Web, Streaming, Gaming, Messaging/VoIP, Social, Cloud/Storage, Software Updates, VPN/Tunnels, DNS, Email, Network Services and Other.
- **FR-010**: Rules MUST be able to match on protocol, local/remote port or port range, remote IP/CIDR, ASN, country, local host, interface, and (when available) remote hostname pattern.
- **FR-011**: Users MUST be able to create, edit, reorder, enable/disable and delete category rules and categories, and see how many flows each rule matched in the last 24 hours.
- **FR-012**: Users MUST be able to assign friendly names to local hosts by IP or MAC address.
- **FR-013**: System MUST optionally poll the router read-only for DHCP leases and DNS cache entries to name local hosts and remote endpoints; this integration MUST be off by default.

**Viewing & analysis**

- **FR-014**: System MUST provide a dashboard with throughput over time, and breakdowns by category, protocol, direction, country and ASN for a selectable time range.
- **FR-015**: System MUST provide ranked "top N" views for local hosts, remote endpoints, remote ASNs, countries, ports and categories.
- **FR-016**: System MUST provide per-host drill-down (time series, categories, top remote endpoints, protocols).
- **FR-017**: Users MUST be able to filter every view by any combination of host, category, protocol, direction, country, ASN and exporter/interface.
- **FR-018**: System MUST provide a raw flow search with filters, paging and CSV export (max 100,000 rows per export).
- **FR-019**: System MUST show a collection status page (per exporter: protocol/version, flows per second, last seen, sampling rate, error and drop counters) and a global warning banner when collection has stalled for 5 minutes.

**Storage & retention**

- **FR-020**: System MUST retain raw flow records for a configurable period (default 7 days).
- **FR-021**: System MUST retain pre-aggregated summaries at 1-minute resolution for a configurable period (default 30 days) and at 1-hour resolution for a configurable period (default 1 year).
- **FR-022**: System MUST enforce a configurable disk usage cap, removing oldest raw data first when exceeded.
- **FR-023**: System MUST be able to re-categorize raw flows still within retention after rule changes and rebuild the affected summaries.

**Operation & access**

- **FR-024**: System MUST be single-user: one access password (or token) protects the web UI and its data interface; there are no user accounts or roles.
- **FR-025**: System MUST be deployable as a single self-contained service plus at most one optional companion process, with configuration via a single file and environment variables.
- **FR-026**: System MUST persist all user settings (rules, names, local networks, retention) across restarts and support exporting/importing them as a file.

### Key Entities

- **Exporter**: A router/device sending flows. Address, protocol/version, observation domain, sampling rate, last seen, counters (received, parsed, dropped, errors).
- **Flow Record**: One reported flow interval. Start/end time, exporter, input/output interface, IP version, local and remote address, ports, protocol, TCP flags, bytes, packets (sampling-scaled), direction, category, rule matched, remote country, ASN, AS name, remote hostname (if known).
- **Traffic Summary**: Pre-aggregated totals of bytes/packets/flows per time bucket (1 min / 1 h) per combination of category, direction, protocol, local host, country, ASN and exporter.
- **Category**: Named traffic class with colour and description; built-in or user-defined.
- **Category Rule**: Ordered match criteria (see FR-010) → category; priority, enabled flag, origin (built-in/user), hit counter.
- **Local Host**: IP and/or MAC of a device on the local network, with friendly name, router-provided hostname and first/last seen.
- **Local Network**: CIDR range considered "inside" for direction classification.
- **Settings**: Listening ports, exporter allow-list, retention periods, disk cap, router integration settings, access password.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user with a MikroTik router can go from installation to seeing categorized traffic in the dashboard in under 10 minutes following the documentation.
- **SC-002**: The system sustains 10,000 flow records per second continuously with zero loss on a machine with 2 CPU cores and 2 GB RAM, while total memory use stays below 1 GB.
- **SC-003**: Traffic appears in the dashboard within 30 seconds of the router reporting it.
- **SC-004**: Dashboard and top-N views for any range up to 30 days load in under 2 seconds; raw flow searches over 24 hours return the first page in under 3 seconds.
- **SC-005**: Total bytes shown for a time range differ from the router's own interface counters for the same period by less than 2% (unsampled export).
- **SC-006**: In a typical home/small-office traffic mix, at least 80% of bytes are assigned to a category other than "Other" using built-in rules plus router hostname integration.
- **SC-007**: With default retention settings and an average load of 500 flows/s, total stored data stays under 20 GB.
- **SC-008**: The service runs for 30 days without restart, memory growth, or manual maintenance.

## Assumptions

- Target user is a single technically capable person (home lab, small office, small ISP edge) who administers the MikroTik router themselves.
- Typical load is 100–2,000 flows/s, with peaks up to 10,000 flows/s; larger ISP-scale deployments are out of scope for v1.
- Flow records do not carry application-layer data; categorization is heuristic (ports, IP ranges, ASN, hostnames from the router's DNS cache). Deep packet inspection is out of scope.
- Country/ASN data comes from freely redistributable offline databases that the user downloads/updates; no data is sent to third-party services.
- The service runs inside a trusted LAN; TLS is provided by a reverse proxy if the user needs it. Single password protection is sufficient.
- Alerting/notifications (email, push) are out of scope for v1.
- Router configuration is done by the user; tmon documents the required RouterOS commands but does not configure the router.
- The browser UI targets current desktop browsers; mobile layout is responsive but not separately optimized.
