# Contract: RouterOS v7 capture configuration (documented, not applied by tmon)

This is the source for the capture section of `docs/routeros.md` (FR-023). In the commands below, replace `192.168.88.10` with the tmon host. `LAN` and `WAN` are the default RouterOS interface lists.

## 1. Capture rules

```routeros
/ip firewall mangle
add chain=prerouting in-interface-list=LAN connection-bytes=0-6000 \
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

- `connection-bytes=0-6000` limits capture to the first ~6 KB of each connection. Expected cost is about 1–3 Mbit/s towards tmon on a busy home network.
- Rule 1 must **not** match traffic to the tmon host itself, or capture would loop. Add `dst-address=!192.168.88.10` if tmon sits on the LAN.

## 2. FastTrack exemption (FR-021, R2)

Fast-tracked packets skip mangle. Restrict the FastTrack rule to bytes after the first 6 KB:

```routeros
/ip firewall filter
set [find action=fasttrack-connection] connection-bytes=6000-0
/ipv6 firewall filter
set [find action=fasttrack-connection] connection-bytes=6000-0
```

Bulk transfer stays fast-pathed. Only setup packets go through the slow path.

## 3. Verify

1. Run `/ip firewall mangle print stats where comment~"tmon"`: the packet counters increase.
2. On the tmon Status page, the Capture card shows datagrams received and names by source within a minute. There is no `capture_*` warning.
3. Open a website on a LAN device. Within one flow export interval it appears in Flows with a `tls` or `quic` tag.

## 4. Host networking limitation (FR-020)

RouterOS sends full-size frames: up to 1,514 + 4 (TZSP header) + tag bytes, which is about 1,519 bytes in a UDP datagram. WSL2 in mirrored networking mode drops UDP datagrams larger than 1,472 bytes, so most TLS/QUIC handshakes never arrive. tmon detects this (`capture_large_packets_lost`).

Fixes, in order of preference:
1. Run tmon natively on Linux, or in Docker on a Linux host.
2. On Windows, use WSL2 NAT mode with a port proxy, or Docker Desktop with the port published.

There is no router-side truncation option for `sniff-tzsp`.

## 5. Inbound connections

Port-forwarded inbound connections (where the LAN host is the server) are not named by these rules. This is by design (R8).
