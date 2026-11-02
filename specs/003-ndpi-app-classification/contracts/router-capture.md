# Contract: router capture, reply direction (optional, feature 003)

This extends [feature 002's router capture](../../002-connection-naming/contracts/router-capture.md). It is optional, but recommended when application classification is on, because many applications are only recognized from the server's reply (research R5).

Replace `192.168.88.10` with the tmon host.

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
- **Interaction with feature 002**: no change in naming. Reply packets are oriented by local networks and ignored by the naming step.
- **FastTrack**: the `connection-bytes=6000-0` exemption from feature 002 already covers the reply direction.
- **Docs**: this goes into `docs/routeros.md` → *Packet capture* → *Reply direction (for application classification)*.
