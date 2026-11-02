# OpenWrt setup

tmon works with OpenWrt (23.05 / 24.10) for all three inputs:

| tmon input | OpenWrt side | Required? |
|---|---|---|
| Flow statistics | `softflowd` (NetFlow v9 / IPFIX) → UDP 2055 | yes |
| Connection naming and nDPI apps | `port-mirroring` in TZSP mode → UDP 37008 | optional |
| DHCP host names | rpcd/ubus JSON-RPC, `luci-rpc getDHCPLeases` | optional |

OpenWrt's resolver (dnsmasq) has no DNS-cache API. Remote host names therefore come from packet capture: tmon reads them from TLS, QUIC, HTTP and DNS answers.

In the examples, `192.168.1.1` is the router and `192.168.1.101` is the tmon host.

> **Turn off hardware flow offloading** (*Network → Firewall → Routing/NAT Offloading*). Offloaded traffic never reaches the CPU, so softflowd and port-mirroring cannot see it. Software offloading is fine.

## 1. Flow export (softflowd)

```sh
opkg update && opkg install softflowd
uci set softflowd.@softflowd[0].enabled='1'
uci set softflowd.@softflowd[0].interface='br-lan'
uci set softflowd.@softflowd[0].host_port='192.168.1.101:2055'
uci set softflowd.@softflowd[0].export_version='9'   # 9 also carries IPv6; 10 = IPFIX
uci commit softflowd && /etc/init.d/softflowd enable && /etc/init.d/softflowd restart
```

Option names can differ between package versions. Check `/etc/config/softflowd` on your router first. Export from `br-lan`, so flows carry LAN addresses (tmon attributes traffic to local hosts). In `tmon.toml`, add the router's address to `collector.allowed_exporters`.

## 2. Packet capture (port-mirroring, optional)

`port-mirroring` sends copies of packets as TZSP (version 1, Ethernet). That is the same format MikroTik's `sniff-tzsp` uses. The destination port is fixed at **37008**.

```sh
opkg install port-mirroring
uci set port-mirroring.@port-mirroring[0].source_ports='br-lan'
uci set port-mirroring.@port-mirroring[0].target='192.168.1.101'
uci set port-mirroring.@port-mirroring[0].protocol='TZSP'
uci set port-mirroring.@port-mirroring[0].filter='udp port 53 or (udp port 443 and udp[8] & 0x80 != 0) or (tcp port 443 and tcp[((tcp[12]&0xf0)>>2)] = 0x16) or (tcp port 80 and tcp[tcpflags] & tcp-push != 0)'
uci commit port-mirroring && /etc/init.d/port-mirroring enable && /etc/init.d/port-mirroring restart
```

Unlike MikroTik's `connection-bytes=0-6000`, port-mirroring copies **every** matching packet. The filter above keeps only what naming needs: DNS, QUIC long headers, TLS handshake records and HTTP requests. nDPI application classification (P2P, VPN, games) needs the first packets of *every* connection. For that, leave `filter` empty and mirror everything. This roughly doubles LAN traffic through the router CPU, so use it only on capable hardware.

In `tmon.toml`, set `[capture] enabled = true` and add the router's address to `allowed_sources`.

**Docker Desktop on Windows (WSL2 mirrored mode).** UDP 37008 is in a reserved Windows range, so the compose file publishes host port 38008 instead (see `deploy/docker-compose.dpi.yml`). port-mirroring cannot change its port, so rewrite it on the router:

```sh
cat > /etc/nftables.d/90-tmon-tzsp.nft <<'NFT'
chain tmon_tzsp_out {
    type nat hook output priority -100; policy accept;
    ip daddr 192.168.1.101 udp dport 37008 dnat ip to 192.168.1.101:38008
}
NFT
fw4 reload
```

## 3. DHCP host names (rpcd, optional)

tmon logs in to `/ubus` and calls **only** `luci-rpc getDHCPLeases`. This requires `luci-base` (it provides `rpcd-mod-luci`), which is installed with LuCI.

Create an ACL that allows nothing else:

```sh
cat > /usr/share/rpcd/acl.d/tmon.json <<'JSON'
{
  "tmon-leases": {
    "description": "tmon: read DHCP leases only",
    "read": { "ubus": { "luci-rpc": [ "getDHCPLeases" ] } }
  }
}
JSON
```

Then create a login that uses that ACL. The password hash comes from `uhttpd -m`, so no system account is needed. `read -s` keeps the password out of shell history:

```sh
read -s -p 'tmon password: ' PW; echo
HASH=$(uhttpd -m "$PW"); unset PW
uci add rpcd login
uci set rpcd.@login[-1].username='tmon'
uci set rpcd.@login[-1].password="$HASH"
uci add_list rpcd.@login[-1].read='tmon-leases'
uci commit rpcd && /etc/init.d/rpcd restart
```

Do not use the `"ubus": {"*": ["*"]}` wildcard from forum examples. It grants far more than read access.

Check the login from the tmon host. The second call should return `dhcp_leases`:

```sh
read -s -p 'tmon password: ' PW; echo
TOKEN=$(curl -s http://192.168.1.1/ubus -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"call\",\"params\":[\"00000000000000000000000000000000\",\"session\",\"login\",{\"username\":\"tmon\",\"password\":\"$PW\"}]}" | sed -n 's/.*"ubus_rpc_session":"\([0-9a-f]*\)".*/\1/p'); unset PW
curl -s http://192.168.1.1/ubus -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"call\",\"params\":[\"$TOKEN\",\"luci-rpc\",\"getDHCPLeases\",{}]}"
```

In tmon, open *Settings → Router integration* and set:

- **Router type:** OpenWrt
- **URL:** preferably `https://192.168.1.1` (install `luci-ssl`). With plain `http://`, the login password crosses the LAN unencrypted. With the self-signed certificate, turn *Verify TLS* off.
- **User:** `tmon` and the password

Then click *Test connection* and save. Alternatively, set it in `tmon.toml`:

```toml
[router]
kind = "openwrt"
enabled = true
url = "http://192.168.1.1"
username = "tmon"
verify_tls = true
lease_poll_secs = 60
# password: use the env var TMON_ROUTER__PASSWORD
```

Errors shown on the Status page:

| Error | Meaning |
|---|---|
| `router rejected the credentials` | Wrong user or password, or the `rpcd` login entry is missing. |
| `router denied access: luci-rpc.getDHCPLeases not permitted by the rpcd ACL` | The ACL file is missing or not listed in `read`. Restart rpcd after adding it. |
| `unexpected response … ubus status` / `no result` | `luci-base` is not installed, so `luci-rpc` does not exist. |

tmon never follows HTTP redirects from the router, so a redirect cannot send the login to another host. tmon keeps the session token and logs in again automatically when rpcd expires it (after 300 s by default).
