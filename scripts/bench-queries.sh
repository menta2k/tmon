#!/usr/bin/env bash
# Query latency benchmark (quickstart V14, SC-004).
# Seeds 30 days of rollups + 24 h of raw data, then asserts dashboard endpoints over 30 d
# answer in < 2 s and the first page of 24 h raw flow search in < 3 s.
#   scripts/bench-queries.sh [FLOWS_PER_HOUR=10000]
set -euo pipefail

PER_HOUR="${1:-10000}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REL="$ROOT/backend/target/release"
WORK="$(mktemp -d)"
TOKEN="bench-$(head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n')"
HTTP=127.0.0.1:18081
API="http://$HTTP/api/v1"

cleanup() { [[ -n "${PID:-}" ]] && kill "$PID" 2>/dev/null || true; wait 2>/dev/null || true; rm -rf "$WORK"; }
trap cleanup EXIT

(cd "$ROOT/backend" && cargo build --release --workspace --bins --features tmon/embed-ui -q)
"$REL/seed_rollups" "$WORK/data" "$PER_HOUR" 30

HASH="$(printf %s "$TOKEN" | "$REL/tmon" hash-password)"
cat >"$WORK/tmon.toml" <<TOML
data_dir = "$WORK/data"
[http]
listen = "$HTTP"
api_token = "$TOKEN"
password_hash = "$HASH"
[collector]
netflow_listen = []
sflow_listen = []
TOML
"$REL/tmon" serve --config "$WORK/tmon.toml" >"$WORK/tmon.log" 2>&1 &
PID=$!
for _ in $(seq 100); do curl -fs "$API/health" >/dev/null && break; sleep 0.2; done
curl -fs "http://$HTTP/api/v1/health" >/dev/null || { echo "tmon did not start:"; cat "$WORK/tmon.log"; exit 1; }

FAIL=0
check() { # name, limit_seconds, path
  local t
  t="$(curl -fs -o /dev/null -w '%{time_total}' -H "Authorization: Bearer $TOKEN" "$API$3")" || { echo "FAIL $1: request error"; FAIL=1; return; }
  if awk -v t="$t" -v l="$2" 'BEGIN {exit !(t < l)}'; then echo "ok   $1 ${t}s (< $2 s)"; else echo "FAIL $1 ${t}s (≥ $2 s)"; FAIL=1; fi
}

R30="from=now-30d&to=now"
for pass in cold warm; do
  echo "-- $pass"
  check "timeseries/category 30d" 2 "/traffic/timeseries?$R30&group_by=category"
  check "breakdown/category 30d"  2 "/traffic/breakdown?$R30&by=category"
  check "breakdown/country 30d"   2 "/traffic/breakdown?$R30&by=country"
  check "top/hosts 30d"           2 "/top/hosts?$R30&limit=20"
  check "top/remotes 30d"         2 "/top/remotes?$R30&limit=20"
  check "top/asns 30d"            2 "/top/asns?$R30&limit=20"
  check "top/ports 30d"           2 "/top/ports?$R30&limit=20"
  check "flows 24h page 1"        3 "/flows?from=now-24h&to=now&limit=100"
  check "flows 24h filtered"      3 "/flows?from=now-24h&to=now&port=443&limit=100"
done
(( FAIL == 0 )) && echo "PASS"
exit "$FAIL"
