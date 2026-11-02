# Generating pcap fixtures

The fixtures in `backend/tests/fixtures/` are produced deterministically by

```bash
cd backend && cargo run -p tmon-collector --example gen_fixtures -- tests/fixtures
```

See `backend/tests/fixtures/README.md` for what each file contains. To capture a real RouterOS export, set `/ip traffic-flow target` to the capturing host and run `tcpdump -w out.pcap udp port 2055`. Then replay it with `tmon replay out.pcap --to 127.0.0.1:2055`.
