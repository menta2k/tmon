# Contract: Collector ports & configuration file

## Inbound UDP (collection, FR-001/FR-002/FR-005)

| Listener | Default bind | Accepts |
|---|---|---|
| `netflow` | `0.0.0.0:2055` | NetFlow v5, v9 and IPFIX (version auto-detected from the header) |
| `ipfix` | `0.0.0.0:4739` | IPFIX (alias of the above, the IANA port) |
| `sflow` | `0.0.0.0:6343` | sFlow v5 |
| HTTP | `0.0.0.0:8080` | UI, API, `/metrics` |

Datagrams from addresses outside `collector.allowed_exporters` (if it is non-empty) are dropped and counted under `dropped{reason="not_allowed"}`.

## `tmon.toml` (every key can be overridden by `TMON_<SECTION>__<KEY>`)

```toml
data_dir = "/var/lib/tmon"

[http]
listen = "0.0.0.0:8080"
# argon2 hash; generate with `tmon hash-password`. TMON_HTTP__PASSWORD_HASH env preferred.
password_hash = ""
api_token = ""                 # optional, for /metrics and scripts
session_ttl_hours = 168

[collector]
netflow_listen = ["0.0.0.0:2055", "0.0.0.0:4739"]
sflow_listen = ["0.0.0.0:6343"]
allowed_exporters = []         # e.g. ["192.168.88.1"]; empty = accept all
socket_recv_buffer_bytes = 8388608
queue_capacity = 65536

[storage]
flush_interval_secs = 5
memory_limit = "512MB"
threads = 2
disk_cap_gb = 20

[retention]
raw_days = 7
rollup_1m_days = 30
rollup_1h_days = 365

[geoip]
country_db = "/var/lib/tmon/geo/dbip-country-lite.mmdb"
asn_db = "/var/lib/tmon/geo/dbip-asn-lite.mmdb"

[router]                       # optional integration (FR-013), off by default
enabled = false
url = "https://192.168.88.1"
username = "tmon-ro"
password = ""                  # use TMON_ROUTER__PASSWORD
verify_tls = true
lease_poll_secs = 60
dns_poll_secs = 30
```

**Startup validation**: `tmon` refuses to start, with a clear message, if `password_hash` is empty (it prints the `tmon hash-password` hint), if a listen address does not parse, or if `data_dir` is not writable. A missing GeoIP DB is a warning, not a fatal error.

## CLI

```text
tmon serve [--config tmon.toml]
tmon hash-password                 # reads password from stdin, prints argon2 hash
tmon geoip-update                  # downloads current DB-IP Lite mmdb files
tmon replay <file.pcap> [--to 127.0.0.1:2055] [--speed 1x|max]   # dev/test aid
```

## Recommended RouterOS v7 configuration (documented, not applied by tmon)

```routeros
/ip traffic-flow set enabled=yes interfaces=WAN active-flow-timeout=1m inactive-flow-timeout=15s
/ip traffic-flow ipfix set nat-src-address=yes nat-src-port=yes nat-dst-address=yes nat-dst-port=yes   # verified against MikroTik docs (docs/routeros.md)
/ip traffic-flow target add dst-address=<tmon-ip> port=2055 version=ipfix
# optional read-only REST user for FR-013
/user group add name=tmon-ro policy=read,api,rest-api
/user add name=tmon-ro group=tmon-ro password=<secret>
```
