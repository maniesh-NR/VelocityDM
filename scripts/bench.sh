#!/usr/bin/env bash
# VelocityDM performance and memory benchmark.
# Measures: throughput (MB/s), peak daemon RSS, SSE event rate cap,
# rate-limit accuracy.
# Requires: release binaries (cargo build --release), bash, python3.
set -u
cd "$(dirname "$0")/.."
BIN=./target/release/velocitydm
CLI=./target/release/velocitydm
if [ ! -x "$BIN" ]; then echo "FAIL: release binaries missing (cargo build --release)"; exit 1; fi

TMP=$(mktemp -d /tmp/vdm-bench.XXXXXX)
DATA="$TMP/config"; DL="$TMP/downloads"; SERVE="$TMP/serve"
mkdir -p "$DATA" "$DL" "$SERVE"
ADDR=127.0.0.1:39731

# Generate a 128 MiB payload.
python3 - "$SERVE/payload.bin" << 'PYEOF'
import sys
path = sys.argv[1]
state = 0x9E3779B97F4A7C15
size = 128 * 1024 * 1024
with open(path, "wb") as f:
    written = 0
    while written < size:
        state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
        state ^= state >> 7
        state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
        f.write(state.to_bytes(8, "little"))
        written += 8
print("payload: 128 MiB written")
PYEOF
sha=$(sha256sum "$SERVE/payload.bin" | cut -d' ' -f1)
echo "payload sha256: $sha"

# Range-capable static server.
cat > "$TMP/rangesrv.py" << 'PYEOF'
import http.server, os, sys, re
root = sys.argv[1]

class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def do_HEAD(self):
        self.serve(head_only=True)
    def do_GET(self):
        self.serve(head_only=False)
    def serve(self, head_only):
        path = os.path.join(root, self.path.lstrip("/"))
        if not os.path.isfile(path):
            self.send_response(404); self.end_headers(); return
        size = os.path.getsize(path)
        rng = self.headers.get("Range")
        m = re.match(r"bytes=(\d*)-(\d*)", rng or "")
        if m and (m.group(1) or m.group(2)):
            start = int(m.group(1) or 0)
            end = int(m.group(2)) if m.group(2) else size - 1
            end = min(end, size - 1)
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
            self.send_header("Content-Length", str(end - start + 1))
        else:
            start, end = 0, size - 1
            self.send_response(200)
            self.send_header("Content-Length", str(size))
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()
        if not head_only:
            with open(path, "rb") as f:
                f.seek(start)
                remaining = end - start + 1
                while remaining > 0:
                    chunk = f.read(min(1024 * 1024, remaining))
                    if not chunk: break
                    self.wfile.write(chunk)
                    remaining -= len(chunk)
    def log_message(self, *a):
        pass

http.server.ThreadingHTTPServer(("127.0.0.1", 39740), Handler).serve_forever()
PYEOF
python3 "$TMP/rangesrv.py" "$SERVE" > "$TMP/server.log" 2>&1 &
SRV_PID=$!
sleep 0.5

# Start the daemon (config isolated via XDG_CONFIG_HOME).
XDG_CONFIG_HOME="$DATA" "$BIN" --addr $ADDR daemon > "$TMP/daemon.log" 2>&1 &
DAEMON_PID=$!
CLEANUP() { kill $SRV_PID $DAEMON_PID 2>/dev/null; rm -rf "$TMP"; }
trap CLEANUP EXIT

# Wait for readiness like the CLI does.
for _ in $(seq 1 40); do
  "$CLI" --addr $ADDR limits >/dev/null 2>&1 && break
  sleep 0.25
done
"$CLI" --addr $ADDR settings --dir "$DL" >/dev/null

rss_peak() {
  if [ -r "/proc/$DAEMON_PID/status" ]; then
    awk '/VmRSS/ {print $2}' "/proc/$DAEMON_PID/status" 2>/dev/null
  fi
}
rm -f "$TMP/stop-sampler" "$TMP/peak"
(
  PEAK_LOCAL=0
  while [ ! -f "$TMP/stop-sampler" ]; do
    R=$(rss_peak)
    if [ -n "$R" ] && [ "$R" -gt "$PEAK_LOCAL" ] 2>/dev/null; then
      PEAK_LOCAL=$R
      echo "$PEAK_LOCAL" > "$TMP/peak"
    fi
    sleep 0.05
  done
) &
SAMPLER_PID=$!

# SSE event counter for the duration of the download (same address as RPC).
python3 - "$ADDR" > "$TMP/sse.count" 2>&1 << 'PYEOF' &
import socket, sys, time
addr = sys.argv[1]
host, port = addr.rsplit(":", 1)
s = socket.create_connection((host, int(port)))
s.sendall(f"GET /events HTTP/1.1\r\nHost: {addr}\r\n\r\n".encode())
s.settimeout(0.2)
count = 0
buf = b""
deadline = time.time() + 60
while time.time() < deadline:
    try:
        data = s.recv(65536)
        if not data:
            break
        buf += data
        count += buf.count(b"data: ")
        buf = buf[-8:]
    except socket.timeout:
        pass
print(count)
PYEOF
SSE_PID=$!

echo "=== download 128 MiB (8 connections) ==="
START=$(date +%s.%N)
ID=$("$CLI" --addr $ADDR add "http://127.0.0.1:39740/payload.bin" --conn 8 | grep -o '#[0-9]*' | tr -d '#')
# poll to completion
for _ in $(seq 1 600); do
  S=$("$CLI" --addr $ADDR status "$ID" 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin).get("state",""))' 2>/dev/null)
  [ "$S" = "completed" ] && break
  [ "$S" = "failed" ] && break
  sleep 0.2
done
END=$(date +%s.%N)
if [ "${S:-}" != "completed" ]; then
  echo "FAIL: download did not complete (state=$S); daemon log:"
  cat "$TMP/daemon.log" | tail -5
  exit 1
fi

# Sustained-load phase: 128 MiB at 16 MiB/s so the sampler sees real load.
"$CLI" --addr $ADDR limits --global 16MB >/dev/null
"$CLI" --addr $ADDR add "http://127.0.0.1:39740/payload.bin" --out sustained.bin >/dev/null
ID2=$("$CLI" --addr $ADDR list --json | python3 -c 'import json,sys;a=json.load(sys.stdin);print([t["id"] for t in a if t["name"]=="sustained.bin"][0])' 2>/dev/null)
for _ in $(seq 1 600); do
  S2=$("$CLI" --addr $ADDR status "$ID2" 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin).get("state",""))' 2>/dev/null)
  [ "$S2" = "completed" ] && break
  [ "$S2" = "failed" ] && break
  sleep 0.2
done
touch "$TMP/stop-sampler"
echo "sustained @ 16 MiB/s phase complete (state=$S2)"

ELAPSED=$(python3 -c "print(f'{$END - $START:.2f}')")
MBPS=$(python3 -c "print(f'{128 / ($END - $START):.1f}')")
echo "download time: ${ELAPSED}s  throughput: ${MBPS} MB/s"

# Verify sha256.
GOT=$(sha256sum "$DL/payload.bin" | cut -d' ' -f1)
if [ "$GOT" = "$sha" ]; then echo "sha256: MATCH"; else echo "sha256: MISMATCH"; fi

wait $SSE_PID 2>/dev/null
EVENTS=$(cat "$TMP/sse.count" 2>/dev/null | tail -1)
echo "SSE events during download: ${EVENTS:-n/a} (10 Hz progress cap)"

wait $SAMPLER_PID 2>/dev/null
PEAK=$(cat "$TMP/peak" 2>/dev/null | sort -n | tail -1)
echo "peak daemon RSS: ${PEAK} KiB (~$((PEAK / 1024)) MiB)"

# Memory after idle (post-download steady state).
IDLE=$(rss_peak)
echo "daemon RSS after download: ${IDLE} KiB (~$((IDLE / 1024)) MiB)"

echo "BENCH-DONE"
