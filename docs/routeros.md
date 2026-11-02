# Configuring RouterOS v7 for tmon

tmon receives flow exports from the router over UDP. The router integration (DHCP and DNS names) is optional and uses a separate read-only REST login.

Below, `<tmon-ip>` is the address of the host running tmon, as seen from the router.

## 1. Flow export (required)

### IPFIX / NetFlow (Traffic Flow) — recommended

```routeros
/ip traffic-flow
set enabled=yes interfaces=WAN cache-entries=128k active-flow-timeout=1m inactive-flow-timeout=15s
/ip traffic-flow target
add dst-address=<tmon-ip> port=2055 version=ipfix
# make sure the NAT fields are exported (post-NAT address/port)
/ip traffic-flow ipfix
set nat-src-address=yes nat-src-port=yes nat-dst-address=yes nat-dst-port=yes
```

Property names were checked against the MikroTik *Traffic Flow* documentation (RouterOS v7):
- `/ip traffic-flow`: `interfaces` (default `all`), `cache-entries` (`4k`), `active-flow-timeout` (`30m`), `inactive-flow-timeout` (`15s`), `packet-sampling`.
- `/ip traffic-flow target`: `dst-address`, `port` (`2055`), `version` (`1`, `5`, `9`, `IPFIX`), `v9-template-refresh` (`20`).
- `/ip traffic-flow ipfix`: per-field toggles, including `nat-src-address`, `nat-src-port`, `nat-dst-address`, `nat-dst-port` and `nat-events`.

- **`interfaces`:** list only the WAN (internet-facing) interface(s), e.g. `interfaces=ether1` or an interface list. Exporting both LAN and WAN counts every flow twice.
  - If you must export several interfaces, set **Count direction = ingress** for the exporter in tmon (*Status → Exporters*). Duplicates are then dropped using the IPFIX `flowDirection` field.
- **`active-flow-timeout=1m`:** keeps long downloads visible in near real time. The RouterOS default is 30 min, which makes graphs lumpy.
- **Versions:** `version=9` (NetFlow v9) and `version=5` work too. IPFIX is preferred because it carries IPv6 and NAT fields.
- **NAT:** IPFIX includes post-NAT addresses (`postNATSourceIPv4Address` etc.). tmon stores them as `nat_ip`/`nat_port` and always classifies on the inside (pre-NAT) address.

Check that it works:

```routeros
/ip traffic-flow print
/ip traffic-flow target print stats
```

In tmon, *Status → Exporters* should show the router within a minute, with `parse errors = 0`.

### sFlow (switch chips, CRS) — optional

```routeros
/interface ethernet switch
set switch1 sflow-enabled=yes            # model dependent; see your switch docs
/interface/sflow
set enabled=yes sampling-rate=512 target-address=<tmon-ip> target-port=6343 agent-address=<router-ip>
```

sFlow is sampled: tmon multiplies bytes and packets by the sampling rate and labels the data **estimated (sampled)**. To correct a wrong advertised rate, use the per-exporter *Sampling override*.

## 2. Router integration (optional, FR-013)

This adds device names from DHCP leases and remote hostnames from the DNS cache. It enables hostname rules such as `*.googlevideo.com → Streaming`.

Create a **read-only** user that can only use the REST API:

```routeros
/user group add name=tmon-ro policy=read,api,rest-api,!write,!policy,!sensitive,!ftp,!reboot,!test,!sniff,!password,!romon
/user add name=tmon-ro group=tmon-ro password=<long-random-secret> address=<tmon-ip>/32
/ip service set www-ssl disabled=no certificate=<your-cert>   # REST runs over HTTPS (www-ssl)
```

Then either use *Settings → Router integration* in tmon (URL `https://<router-ip>`, user `tmon-ro`, password, then *Test connection*), or set it in the config file:

```toml
[router]
enabled = true
url = "https://192.168.88.1"
username = "tmon-ro"
verify_tls = true          # set false only for a self-signed router certificate on a trusted LAN
lease_poll_secs = 60
dns_poll_secs = 30
# password: use the env var TMON_ROUTER__PASSWORD instead of the file
```

tmon only performs `GET /rest/ip/dhcp-server/lease` and `GET /rest/ip/dns/cache`. It never writes to the router. If the router is unreachable, a warning banner appears and flow collection carries on.

## 3. Firewall

If the router filters output or forward traffic towards tmon, allow UDP 2055 (IPFIX/NetFlow), 6343 (sFlow) and, with packet capture, 37008 (TZSP) to `<tmon-ip>`. If tmon itself is behind a host firewall, open the same ports there (and TCP 8080 for the UI).

## Packet capture (connection naming)

*Optional (feature 002).* Without it, tmon only learns host names from the router's DNS cache, which names about 1 in 5 flows. With capture on, the router copies the **first ~6 KB of every connection** to tmon over TZSP (UDP 37008). tmon reads the server name from the TLS ClientHello, from the QUIC Initial packet (HTTP/3), from the HTTP `Host` header and from DNS answers. Hostname rules then match with *detected* confidence. tmon keeps no payload: only names stay in memory, and they expire after 10 minutes.

Enable the listener in `tmon.toml` (see `deploy/tmon.example.toml`):

```toml
[capture]
enabled = true
listen = "0.0.0.0:37008"
allowed_sources = ["192.168.88.1"]   # empty = falls back to collector.allowed_exporters
```

### Capture rules

Replace `192.168.88.10` with the tmon host.

```routeros
/ip firewall mangle
add chain=prerouting in-interface-list=LAN connection-bytes=0-6000 dst-address=!192.168.88.10 \
    action=sniff-tzsp sniff-target=192.168.88.10 sniff-target-port=37008 \
    passthrough=yes comment="tmon: connection setup (TLS/QUIC/HTTP)"
add chain=output protocol=udp src-port=53 \
    action=sniff-tzsp sniff-target=192.168.88.10 sniff-target-port=37008 \
    passthrough=yes comment="tmon: DNS answers from router resolver"
add chain=prerouting in-interface-list=WAN protocol=udp src-port=53 \
    action=sniff-tzsp sniff-target=192.168.88.10 sniff-target-port=37008 \
    passthrough=yes comment="tmon: DNS answers from external resolvers"

/ipv6 firewall mangle
add chain=prerouting in-interface-list=LAN connection-bytes=0-6000 \
    action=sniff-tzsp sniff-target=192.168.88.10 sniff-target-port=37008 \
    passthrough=yes comment="tmon: connection setup (IPv6)"
add chain=output protocol=udp src-port=53 \
    action=sniff-tzsp sniff-target=192.168.88.10 sniff-target-port=37008 \
    passthrough=yes comment="tmon: DNS answers (IPv6)"
```

- `connection-bytes=0-6000` limits capture to connection setup. Expect about **1–3 Mbit/s** towards tmon on a busy home network.
- **Avoid a capture loop**: the first rule must not match traffic to the tmon host itself. Keep `dst-address=!<tmon-ip>` when tmon sits on the LAN.

### FastTrack exemption

Fast-tracked packets skip mangle, and a ClientHello is sent after the connection is *established*. With the default FastTrack rule, tmon would see SYNs but no names (`capture_fastpath_suspected`). Fast-path only the bytes after the first 6 KB:

```routeros
/ip firewall filter
set [find action=fasttrack-connection] connection-bytes=6000-0
/ipv6 firewall filter
set [find action=fasttrack-connection] connection-bytes=6000-0
```

Bulk transfers stay fast-pathed. Only connection setup takes the slow path. Watch the router CPU for a day after the change.

### Reply direction (for application classification)

*Optional; recommended with [application classification](dpi.md) (feature 003).* Many applications are recognized only from the server's reply. Examples are the WireGuard handshake response, the RDP connection confirm, and the second half of the BitTorrent handshake. These rules mirror the first 6 KB of each connection's replies as well:

```routeros
/ip firewall mangle
add chain=forward in-interface-list=WAN out-interface-list=LAN connection-bytes=0-6000 \
    action=sniff-tzsp sniff-target=192.168.88.10 sniff-target-port=37008 \
    passthrough=yes comment="tmon: connection setup, reply direction (DPI)"
/ipv6 firewall mangle
add chain=forward in-interface-list=WAN out-interface-list=LAN connection-bytes=0-6000 \
    action=sniff-tzsp sniff-target=192.168.88.10 sniff-target-port=37008 \
    passthrough=yes comment="tmon: connection setup, reply direction (DPI, IPv6)"
```

- **Cost**: roughly doubles the capture bandwidth, to about 2–6 Mbit/s on a busy home network.
- **Naming is unchanged**: reply packets are oriented by tmon's local networks and ignored by the naming step.
- **FastTrack**: the `connection-bytes=6000-0` exemption above already covers the reply direction.

### Verify

1. `/ip firewall mangle print stats where comment~"tmon"`: the packet counters increase.
2. On the tmon **Status** page, the *Packet capture* card shows datagrams received and names by source within a minute. There is no `capture_*` warning.
3. Open a website on a LAN device. Within one flow export interval it appears in **Flows** with a `TLS` or `QUIC` tag next to the host name.

### WSL2 and large capture packets

RouterOS sends full-size frames: up to 1,514 bytes plus the TZSP header, so about 1,519 bytes per UDP datagram. **WSL2 in mirrored networking mode drops UDP datagrams larger than 1,472 bytes**, so most TLS/QUIC handshakes never arrive. tmon detects this and raises `capture_large_packets_lost`. `sniff-tzsp` has no router-side truncation option. The fixes, in order of preference:

1. Run tmon natively on Linux, or in Docker on a Linux host (`-p 37008:37008/udp`).
2. On Windows, use WSL2 NAT mode with a port proxy, or Docker Desktop with the port published.

**Docker Desktop with WSL2 `networkingMode=mirrored`:** Windows reserves the WSL Linux ephemeral port range for WSL (`/proc/sys/net/ipv4/ip_local_port_range`, 33700–37795 by default). 37008 is inside that range, so publishing it fails with `ports are not available … bind: Only one usage of each socket address`. Publish a host port outside the range instead. `deploy/docker-compose.dpi.yml` uses `38008:37008/udp`. Then set `sniff-target-port=38008` on the router.

### Warnings

| Code | Meaning | Fix |
|---|---|---|
| `capture_silent` | Capture is on, but no packet arrived for 5 min | Check the mangle rules, `sniff-target`, firewalls, and `capture.allowed_sources` |
| `capture_large_packets_lost` | Many handshakes are incomplete and no datagram exceeded 1,472 bytes | See *WSL2 and large capture packets* |
| `capture_fastpath_suspected` | ≥ 100 SYNs in 15 min but < 10 % had their first payload captured | See *FastTrack exemption* |
| `collector_down:37008` | tmon could not bind the capture port | Port already in use, or a wrong `capture.listen` address |

### Limitations

- **Inbound connections** (port forwards, where the LAN host is the server) are not named by these rules. This is by design.
- **Encrypted Client Hello (ECH)**: only the provider's public "outer" name is visible (for example `cloudflare-ech.com`). tmon uses it only as a last resort.
- **Restrict who can reach UDP 37008.** `capture.allowed_sources` checks the UDP source address, which can be spoofed. With no allow-list (and no `collector.allowed_exporters`), every sender is accepted and tmon logs a warning at startup. Allow the port only from the router (host firewall or a management VLAN). Forged captured DNS answers never overwrite names from the router's DNS cache.
- Names learned from captured DNS are bound **per device**, so two devices that resolved different sites on a shared CDN address each get their own name.

## Troubleshooting

| Symptom | Check |
|---|---|
| Exporter never appears | `/ip traffic-flow target print stats` shows packets sent; host firewall; `collector.allowed_exporters` in tmon |
| `template missing` keeps rising | Normal for the first minute after a restart (templates are resent periodically); persistent → firewall dropping fragments |
| Traffic counted twice | Exporting both LAN and WAN interfaces → export WAN only, or set *Count direction* |
| Totals far too high on sFlow | Wrong advertised sampling rate → set *Sampling override* |
| Hosts have no names / everything is "guessed" | Enable [packet capture](#packet-capture-connection-naming); check the Status page capture card |
| tmon in WSL2/Docker on a desktop | The router must reach the collector's UDP port: use WSL "mirrored" networking, Docker `-p 2055:2055/udp`, or run tmon on a LAN host |
