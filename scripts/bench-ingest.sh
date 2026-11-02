#!/usr/bin/env bash
# Sustained-ingest benchmark (quickstart V13, SC-002).
# Runs tmon in a 2 GB / 2-CPU cgroup, floods it with NetFlow v5 at RATE flows/s for SECS,
# then asserts no backpressure drops and peak RSS < 1 GB.
#   scripts/bench-ingest.sh [RATE=10000] [SECS=600]
# Feature 002 (SC-004/SC-005):
#   CAPTURE=1               enable the [capture] listener (idle unless CAPTURE_RATE is set)
#   CAPTURE_RATE=<dgram/s>  also flood the capture port with a TLS/QUIC mix during the run
# Feature 003 (SC-003/SC-005):
#   DPI=1                   also enable [dpi] (implies CAPTURE=1) with the pure-Rust fake_dpi
#                           classifier, or the real sidecar when TMON_DPI_BIN is set; reports
#                           the classifier counters and sidecar peak RSS
set -euo pipefail

RATE="${1:-10000}"
SECS="${2:-600}"
RSS_LIMIT_KB=$((1024 * 1024))
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/backend/target/release/tmon"
WORK="$(mktemp -d)"
TOKEN="bench-$(head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n')"
HTTP=127.0.0.1:18080
NF=127.0.0.1:12055
CAP=127.0.0.1:13700
DPI="${DPI:-0}"
CAPTURE="${CAPTURE:-0}"
[[ "$DPI" == "1" ]] && CAPTURE=1
CAPTURE_RATE="${CAPTURE_RATE:-0}"

cleanup() { [[ -n "${PID:-}" ]] && kill "$PID" 2>/dev/null || true; wait 2>/dev/null || true; rm -rf "$WORK"; }
trap cleanup EXIT

(cd "$ROOT/backend" && cargo build --release --workspace --bins --examples --features tmon/embed-ui,tmon-capture/fixtures -q)

HASH="$(printf %s "$TOKEN" | "$BIN" hash-password)"
cat >"$WORK/tmon.toml" <<TOML
data_dir = "$WORK/data"
[http]
listen = "$HTTP"
api_token = "$TOKEN"
password_hash = "$HASH"
[collector]
netflow_listen = ["$NF"]
sflow_listen = []
TOML
if [[ "$CAPTURE" == "1" ]]; then
  printf '[capture]\nenabled = true\nlisten = "%s"\n' "$CAP" >>"$WORK/tmon.toml"
fi
if [[ "$DPI" == "1" ]]; then
  DPI_BIN="${TMON_DPI_BIN:-$ROOT/backend/target/release/examples/fake_dpi}"
  [[ -x "$DPI_BIN" ]] || { echo "DPI=1: classifier binary not found: $DPI_BIN"; exit 1; }
  printf '[dpi]\nenabled = true\nbinary = "%s"\n' "$DPI_BIN" >>"$WORK/tmon.toml"
fi

if command -v systemd-run >/dev/null && systemd-run --user --scope -q true 2>/dev/null; then
  systemd-run --user --scope -q -p MemoryMax=2G -p CPUQuota=200% "$BIN" serve --config "$WORK/tmon.toml" >"$WORK/tmon.log" 2>&1 &
else
  echo "warn: systemd-run --user unavailable; running without cgroup limits (taskset 2 CPUs)" >&2
  taskset -c 0,1 "$BIN" serve --config "$WORK/tmon.toml" >"$WORK/tmon.log" 2>&1 &
fi
for _ in $(seq 50); do curl -fs "http://$HTTP/api/v1/health" >/dev/null && break; sleep 0.2; done
curl -fs "http://$HTTP/api/v1/health" >/dev/null || { echo "tmon did not start:"; cat "$WORK/tmon.log"; exit 1; }
PID="$(pgrep -n -f "tmon serve --config $WORK/tmon.toml")"

if [[ "$CAPTURE" == "1" && "$CAPTURE_RATE" != "0" ]]; then
  "$ROOT/backend/target/release/examples/capture_flood" "$CAP" "$CAPTURE_RATE" "$SECS" &
  CAP_PID=$!
fi
"$ROOT/backend/target/release/examples/flood" "$NF" "$RATE" "$SECS"
[[ -n "${CAP_PID:-}" ]] && wait "$CAP_PID"
sleep 10 # let the writer flush

metric() { curl -fs -H "Authorization: Bearer $TOKEN" "http://$HTTP/metrics" | awk -v m="$1" '$1 == m {print $2}'; }
DROPPED="$(metric 'tmon_flows_dropped_total{reason="backpressure"}')"
INGESTED="$(curl -fs -H "Authorization: Bearer $TOKEN" "http://$HTTP/metrics" | awk '/^tmon_flows_received_total/ {s += $2} END {print s}')"
PEAK_KB="$(awk '/VmHWM/ {print $2}' "/proc/$PID/status")"

echo "ingested=${INGESTED:-?} dropped_backpressure=${DROPPED:-?} peak_rss=$((PEAK_KB / 1024))MB capture=$CAPTURE capture_rate=$CAPTURE_RATE"
if [[ "$CAPTURE" == "1" ]]; then
  echo "capture: datagrams=$(metric tmon_capture_datagrams_total) connections=$(metric tmon_capture_connections) reassembly_bytes=$(metric tmon_capture_reassembly_bytes) dropped=$(metric 'tmon_capture_dropped_total{reason="backpressure"}')"
fi
if [[ "$DPI" == "1" ]]; then
  DPI_PID="$(pgrep -n -P "$PID" || true)"
  DPI_KB="$( [[ -n "$DPI_PID" ]] && awk '/VmHWM/ {print $2}' "/proc/$DPI_PID/status" || echo 0)"
  echo "dpi: state=$(metric tmon_dpi_up) packets=$(metric 'tmon_dpi_packets_total{result="sent"}') dropped_queue=$(metric 'tmon_dpi_packets_total{result="dropped_queue"}') restarts=$(metric tmon_dpi_restarts_total) sidecar_peak_rss=$((DPI_KB / 1024))MB"
fi
FAIL=0
[[ "${DROPPED:-1}" == "0" ]] || { echo "FAIL: backpressure drops"; FAIL=1; }
(( PEAK_KB < RSS_LIMIT_KB )) || { echo "FAIL: RSS ≥ 1 GB"; FAIL=1; }
if [[ "$DPI" == "1" ]] && (( ${DPI_KB:-0} >= 256 * 1024 )); then echo "FAIL: sidecar RSS ≥ 256 MB"; FAIL=1; fi
(( FAIL == 0 )) && echo "PASS"
exit "$FAIL"
