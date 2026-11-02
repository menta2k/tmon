# Test fixtures

All captures here are **synthetic**. Our own encoders generate them (`tmon-collector/src/fixtures/`), so the expected output is known exactly. No real RouterOS traffic is included, and nothing personal or private ends up in the repository. Each capture is a classic pcap (Ethernet/IPv4/UDP) holding export datagrams.

To regenerate (deterministic, base time `2026-01-01T00:00:00Z`):

```bash
cd backend && cargo run -p tmon-collector --example gen_fixtures -- tests/fixtures
```

| File | Protocol | Contents | Used by |
|---|---|---|---|
| `nf5.pcap` | NetFlow v5 | A few IPv4 flows in one datagram | `netflow_decode.rs` |
| `nf9.pcap` | NetFlow v9 | Template + data in one datagram | `netflow_decode.rs` |
| `nf9-data-before-template.pcap` | NetFlow v9 | Data arrives before its template (counted as `template_missing`), then template + data | `netflow_decode.rs`, quickstart V3 |
| `routeros7-ipfix.pcap` | IPFIX | RouterOS 7–style template (IPv4 + IPv6, post-NAT address/port fields) | `netflow_decode.rs`, quickstart V2 |
| `sflow-crs-1in512.pcap` | sFlow v5 | Flow samples with sampling rate 512 (CRS-style switch) | `sflow_decode.rs`, `us1_dashboard.rs` (V4) |
| `malformed.pcap` | mixed | Truncated / garbage datagrams; must not crash and must count `parse_errors` | `netflow_decode.rs`, fuzz seeds |
| `mixed-home.pcap` | IPFIX | Home network mix: web, gaming, DNS, mail, VPN, NTP/other, IPv6 | `us1_dashboard.rs`, `us2_top.rs`, quickstart V5/V6 |
| `mixed-home.truth.csv` | — | Ground truth for `mixed-home.pcap`: `category,direction,local_ip,bytes` | integration tests |
| `steam.pcap` | IPFIX | UDP 27015–27030 game traffic (matched only by a user rule) | US3 tests, quickstart V7/V8 |

Local network in all captures: `192.168.88.0/24` and `fd00::/8`. The exporter is `192.168.88.1`.

**Replacing these with real captures:** record with `tcpdump -i any -w routeros.pcap udp port 2055`. Before committing, scrub the addresses (for example with `tcprewrite --pnat`) and update the truth CSV.
