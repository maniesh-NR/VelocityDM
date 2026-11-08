#!/usr/bin/env bash
# VelocityDM performance and memory benchmark.
# Measures: throughput (MB/s), peak daemon RSS, event rate cap, pool reuse.
# Requires: release binaries, bash, python3 (test file server with ranges).
set -u
cd "$(dirname "$0")/.."
BIN=./target/release/velocitydm
CLI=./target/release/velocitydm
NMH=./target/release/velocitydm-nmh
if [ ! -x "$BIN" ]; then echo "FAIL: release binaries missing (cargo build --release)"; exit 1; fi

TMP=$(mktemp -d /tmp/vdm-bench.XXXXXX)
DATA="$TMP/data"; DL="$TMP/downloads"; SERVE="$TMP/serve"
mkdir -p "$DATA" "$DL" "$SERVE"
PORT_JSON=""
RPC_PORT=39731
SSE_PORT=39732

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
python3 - "$SERVE" 39740 > "$TMP/server.log" 2>&1 &
SRV_PID=$!
sleep 0.5

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
        path = os.path.join(root, os.path.basename(self.path))
        if not os.path.isfile(path):
            self.send_error(404); return
        size = os.path.getsize(path)
        rng = self.headers.get("Range")
        if rng:
            m = re.match(r"bytes=(\d+)-(\d*)", rng)
            start = int(m.group(1)); end = int(m.group(2)) if m.group(2) else size - 1
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Length", str(end - start + 1))
            self.end_headers()
            if not head_only:
                with open(path, "rb") as f:
                    f.seek(start)
                    remaining = end - start + 1
                    while remaining > 0:
                        chunk = f.read(min(1024 * 1024, remaining))
                        if not chunk: break
                        self.wfile.write(chunk); remaining -= len(chunk)
        else:
            self.send_response(200)
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Length", str(size))
            self.end_headers()
            if not head_only:
                with open(path, "rb") as f:
                    while True:
                        chunk = f.read(1024 * 1024)
                        if not chunk: break
                        self.wfile.write(chunk)
    def log_message(self, *a):
        pass

http.server.ThreadingHTTPServer(("127.0.0.1", 39740), Handler).serve_forever()
PYEOF
python3 "$TMP/rangesrv.py" "$SERVE" > "$TMP/server.log" 2>&1 &
SRV_PID=$!
sleep 0.5

# Start daemon with RSS sampling wrapper.
"$BIN" --data-dir "$DATA" daemon --rpc-port $RPC_PORT --sse-port $SSE_PORT --download-dir "$DL" > "$TMP/daemon.log" 2>&1 &
DAEMON_PID=$!
sleep 1.0

rss_peak() {
  if [ -r "/proc/$DAEMON_PID/status" ]; then
    awk '/VmRSS/ {print $2}' "/proc/$DAEMON_PID/status" 2>/dev/null
  fi
}
PEAK=0
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

# SSE event counter for the duration of the download.
python3 - "$SSE_PORT" > "$TMP/sse.count" 2>&1 << 'PYEOF' &
import socket, sys, time
port = int(sys.argv[1])
s = socket.create_connection(("127.0.0.1", port))
s.sendall(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")
s.settimeout(0.2)
count = 0
buf = b""
deadline = time.time() + 30
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

echo "=== download 128 MiB (8 segments) ==="
START=$(date +%s.%N)
OUT=$("$CLI" --data-dir "$DATA" add "http://127.0.0.1:39740/payload.bin" --wait 2>&1)
END=$(date +%s.%N)
SAMPLING=0

OK=$(echo "$OUT" | grep -c "completed:")
if [ "$OK" != "1" ]; then
  echo "FAIL: download did not complete: $OUT"
  kill $SAMPLER_PID $SSE_PID $SRV_PID $DAEMON_PID 2>/dev/null
  exit 1
fi

touch "$TMP/stop-sampler"
PEAK=$(cat "$TMP/peak" 2>/dev/null || echo 0)
# Sustained-load phase: 32 MiB at 16 MiB/s so the sampler sees real load.
"$CLI" --data-dir "$DATA" set global_rate_limit_bps 16777216 > /dev/null
"$CLI" --data-dir "$DATA" add "http://127.0.0.1:39740/payload.bin" --out sustained.bin --wait > /dev/null 2>&1
touch "$TMP/stop-sampler"
PEAK=$(cat "$TMP/peak" 2>/dev/null | sort -n | tail -1)
echo "sustained 32 MiB @ 16 MiB/s phase complete"
ELAPSED=$(python3 -c "print(f'{$END - $START:.2f}')")
MBPS=$(python3 -c "print(f'{128 / ($END - $START):.1f}')")
echo "download time: ${ELAPSED}s  throughput: ${MBPS} MB/s"

# Verify sha256.
GOT=$(sha256sum "$DL/payload.bin" | cut -d' ' -f1)
if [ "$GOT" = "$sha" ]; then echo "sha256: MATCH"; else echo "sha256: MISMATCH"; fi

wait $SSE_PID 2>/dev/null
EVENTS=$(cat "$TMP/sse.count" 2>/dev/null | tail -1)
echo "SSE events during download: ${EVENTS:-n/a} (refresh cap 12 Hz)"

wait $SAMPLER_PID 2>/dev/null
echo "peak daemon RSS: ${PEAK} KiB (~$((PEAK / 1024)) MiB)"

"$CLI" --data-dir "$DATA" stats | python3 -c "import json,sys; s=json.load(sys.stdin); print('pool: connects=%s reuses=%s' % (s['pool']['connects'], s['pool']['reuses']))"

# Memory after idle (post-download steady state).
IDLE=$(rss_peak)
echo "daemon RSS after download: ${IDLE} KiB (~$((IDLE / 1024)) MiB)"

kill $SRV_PID $DAEMON_PID 2>/dev/null
rm -rf "$TMP"
echo "BENCH-DONE"
