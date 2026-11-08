#!/usr/bin/env bash
# VelocityDM performance A/B benchmark.
#
# Measures wall-clock time, TCP connections and HTTP requests for a
# segmented download over a link with simulated per-connection handshake
# RTT (VDM_RTT_MS, applied once per fresh TCP connection on /rtt/ paths).
# This models the WAN cost of TCP/TLS setup that persistent connections
# avoid. Usage: bench-perf.sh <label> <outfile>
set -u
cd "$(dirname "$0")/.."
BIN=./target/release/velocitydm
LABEL=${1:-bench}
OUT=${2:-/tmp/bench.txt}
ADDR=127.0.0.1:36996
PORT=8768
RTT=${VDM_RTT_MS:-40}
WORK=$(mktemp -d /tmp/vdm-bench-XXXX)

cleanup() {
  [ -n "${DAEMON_PID:-}" ] && kill "$DAEMON_PID" 2>/dev/null
  [ -n "${HTTP_PID:-}" ]  && kill "$HTTP_PID" 2>/dev/null
  wait 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

mkdir -p "$WORK/files/rtt" "$WORK/dl" "$WORK/config"
head -c 134217728 /dev/urandom > "$WORK/files/rtt/big.bin" # 128 MiB

VDM_RTT_MS=$RTT python3 "$PWD/scripts/testserver.py" "$WORK/files" $PORT >"$WORK/http.log" 2>&1 &
HTTP_PID=$!
sleep 0.5

XDG_CONFIG_HOME="$WORK/config" $BIN --addr $ADDR daemon >"$WORK/daemon.log" 2>&1 &
DAEMON_PID=$!
for _ in $(seq 1 40); do
  $BIN --addr $ADDR limits >/dev/null 2>&1 && break
  sleep 0.25
done

T0=$(python3 -c 'import time; print(time.time())')
ID=$($BIN --addr $ADDR add "http://127.0.0.1:$PORT/rtt/big.bin" --dir "$WORK/dl" --conn 8 | grep -o '#[0-9]*' | tr -d '#')
for _ in $(seq 1 600); do
  S=$($BIN --addr $ADDR status "$ID" 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin).get("state",""))' 2>/dev/null)
  [ "$S" = "completed" ] && break
  [ "$S" = "failed" ] && { echo "DOWNLOAD FAILED" >> "$OUT"; break; }
  sleep 0.5
done
T1=$(python3 -c 'import time; print(time.time())')
STATS=$(curl -s "http://127.0.0.1:$PORT/__stats")
ELAPSED=$(python3 -c "print(f'{$T1-$T0:.2f}')")
MBPS=$(python3 -c "print(f'{128/($T1-$T0):.1f}')")
CONNS=$(echo "$STATS" | python3 -c 'import json,sys;print(json.load(sys.stdin)["connections"])')
REQS=$(echo "$STATS" | python3 -c 'import json,sys;print(json.load(sys.stdin)["requests"])')
SHA_OK=$( [ "$(sha256sum "$WORK/dl/big.bin" | cut -d' ' -f1)" = "$(sha256sum "$WORK/files/rtt/big.bin" | cut -d' ' -f1)" ] && echo yes || echo no )

{
  echo "label:            $LABEL"
  echo "file:             128 MiB, 8 connections, ${RTT}ms simulated handshake RTT"
  echo "elapsed_s:        $ELAPSED"
  echo "throughput_MBps:  $MBPS"
  echo "tcp_connections:  $CONNS"
  echo "http_requests:    $REQS"
  echo "byte_identical:   $SHA_OK"
} | tee -a "$OUT"
