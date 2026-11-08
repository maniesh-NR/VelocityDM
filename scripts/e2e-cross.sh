#!/usr/bin/env bash
# VelocityDM cross-feature integration test.
#
# Verifies that features which are designed to compose actually compose:
#   1. segmented download x per-task limit x mirrors x checksum x pause/resume
#   2. HLS (AES-128, master playlist, quality) x global limiter
#   3. concurrent HTTP x batch x torrent x magnet under max-active gate + global limit
#   4. NMH extension channel x magnet (addTorrent) and x HTTP (add)
#   5. batch x connection cap x per-task limit
#   6. memory footprint under mixed load
set -u
cd "$(dirname "$0")/.."
BIN=./target/release/velocitydm
NMH=./target/release/velocitydm-nmh
ADDR=127.0.0.1:36993
TRACKER_PORT=36994
HTTP_PORT=8766
WORK=$(mktemp -d /tmp/vdm-cross-XXXX)
FAILS=0
PASS=0

say()  { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
ok()   { printf '\033[1;32mPASS\033[0m %s\n' "$*"; PASS=$((PASS+1)); }
bad()  { printf '\033[1;31mFAIL\033[0m %s\n' "$*"; FAILS=$((FAILS+1)); }
check(){ if [ "$1" = "0" ]; then ok "$2"; else bad "$2"; fi; }

cleanup() {
  [ -n "${DAEMON_PID:-}" ] && kill "$DAEMON_PID" 2>/dev/null
  [ -n "${SEED_PID:-}" ]  && kill "$SEED_PID" 2>/dev/null
  [ -n "${HTTP_PID:-}" ]  && kill "$HTTP_PID" 2>/dev/null
  wait 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

wait_state() { # id state timeout
  local id=$1 state=$2 t=${3:-60}
  for _ in $(seq 1 $((t*4))); do
    s=$($BIN --addr $ADDR status "$id" 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin).get("state",""))' 2>/dev/null)
    [ "$s" = "$state" ] && return 0
    sleep 0.25
  done
  return 1
}

state_now() { # id
  $BIN --addr $ADDR status "$1" 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin).get("state",""))' 2>/dev/null
}

sha() { sha256sum "$1" | cut -d' ' -f1; }

# ---------------------------------------------------------------- test data
say "test data"
mkdir -p "$WORK/files" "$WORK/dl" "$WORK/hls"
head -c 4194304 /dev/urandom > "$WORK/files/combo.bin"     # 4 MiB
for i in 1 2 3; do head -c 131072 /dev/urandom > "$WORK/files/xpic$i.jpg"; done
python3 "$PWD/scripts/testserver.py" "$WORK/files" $HTTP_PORT >"$WORK/http.log" 2>&1 &
HTTP_PID=$!
sleep 0.5

# AES-128 HLS test stream in a realistic CDN layout:
#   files/master.m3u8            -> references "hi/index.m3u8"
#   files/hi/index.m3u8          -> segments "segN.ts" (same dir), key "../key.bin"
#   files/hi/segN.ts             -> AES-128-CBC encrypted
#   files/key.bin                -> 16-byte key
# The "../key.bin" reference exercises RFC 3986 dot-segment normalization.
python3 - "$WORK/files" "$WORK/hls" <<'PYEOF'
import os, subprocess, sys
serve, ref = sys.argv[1], sys.argv[2]
os.makedirs(ref, exist_ok=True)
key = bytes(range(16))
open(os.path.join(serve, "key.bin"), "wb").write(key)
# two variants: hi = 3 x 96 KiB, lo = 3 x 32 KiB
for name, seglen, count in (("hi", 98304, 3), ("lo", 32768, 3)):
    os.makedirs(os.path.join(serve, name), exist_ok=True)
    playlist = ["#EXTM3U", "#EXT-X-TARGETDURATION:10", "#EXT-X-MEDIA-SEQUENCE:0",
                '#EXT-X-KEY:METHOD=AES-128,URI="../key.bin"', ]
    for i in range(count):
        plain = bytes([(j * 7 + i * 13) & 0xFF for j in range(seglen)])
        # encrypt with openssl AES-128-CBC + PKCS7 (our engine strips padding)
        enc = subprocess.run(
            ["openssl", "enc", "-aes-128-cbc", "-K", key.hex(),
             "-iv", i.to_bytes(16, "big").hex()],
            input=plain, capture_output=True, check=True).stdout
        open(os.path.join(serve, name, f"seg{i}.ts"), "wb").write(enc)
        playlist.append("#EXTINF:9.000,")
        playlist.append(f"seg{i}.ts")
        # remember plaintext stream for byte comparison
        with open(os.path.join(ref, f"{name}-plain.ts"), "ab") as fh:
            fh.write(plain)
    playlist.append("#EXT-X-ENDLIST")
    open(os.path.join(serve, name, "index.m3u8"), "w").write("\n".join(playlist) + "\n")
master = """#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720
hi/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=250000,RESOLUTION=640x360
lo/index.m3u8
"""
open(os.path.join(serve, "master.m3u8"), "w").write(master)
print("hls fixtures ready")
PYEOF
[ -f "$WORK/files/master.m3u8" ]; check $? "HLS AES-128 fixtures generated"

# ---------------------------------------------------------------- daemon
say "daemon"
rm -rf "$WORK/config"; mkdir -p "$WORK/config"
XDG_CONFIG_HOME="$WORK/config" $BIN --addr $ADDR daemon >"$WORK/daemon.log" 2>&1 &
DAEMON_PID=$!
for _ in $(seq 1 40); do
  $BIN --addr $ADDR limits >/dev/null 2>&1 && break
  sleep 0.25
done
$BIN --addr $ADDR limits >/dev/null 2>&1
check $? "daemon up and answering RPC"
# default download dir for channel-added tasks (NMH has no --dir)
$BIN --addr $ADDR settings --dir "$WORK/dl" >/dev/null
check $? "daemon download dir configured"

# ------------------------------------------- combo 1: segmented x limit x mirror x checksum x pause/resume
say "combo 1: segmented x per-task limit x mirrors x checksum x pause/resume"
SUM=$(sha "$WORK/files/combo.bin")
URL="http://127.0.0.1:$HTTP_PORT/combo.bin"
CID=$($BIN --addr $ADDR add "$URL" --dir "$WORK/dl" --conn 4 \
        --limit 400KB --mirror "$URL" --checksum "sha256:$SUM" | grep -o '#[0-9]*' | tr -d '#')
sleep 2
S=$(state_now "$CID")
[ "$S" = "downloading" ]; check $? "rate-limited download is in flight (state=$S)"
$BIN --addr $ADDR pause "$CID" >/dev/null
wait_state "$CID" paused 15
check $? "pause took effect mid-flight (bounded latency, state=$(state_now "$CID"))"
[ -f "$WORK/dl/combo.bin.part" ]; check $? "crash-safe .part present while paused"
$BIN --addr $ADDR resume "$CID" >/dev/null
wait_state "$CID" completed 120
check $? "resumed download completed (limit + mirrors + checksum)"
[ "$(sha "$WORK/dl/combo.bin")" = "$SUM" ]; check $? "byte-identical after pause/resume + checksum verified"
[ ! -f "$WORK/dl/combo.bin.part" ] && [ ! -f "$WORK/dl/combo.bin.vdmctl" ]
check $? "no leftover .part/.vdmctl after combo 1"

# ------------------------------------------- combo 2: HLS x quality x global limiter
say "combo 2: HLS (AES-128) x quality x global limiter"
$BIN --addr $ADDR limits --global 1MB >/dev/null
HID=$($BIN --addr $ADDR add "http://127.0.0.1:$HTTP_PORT/master.m3u8" \
        --dir "$WORK/dl" --quality best | grep -o '#[0-9]*' | tr -d '#')
wait_state "$HID" completed 120
check $? "HLS best-variant download completed under global limit"
[ -f "$WORK/dl/master.ts" ]; check $? "HLS assembled output master.ts"
HLS_SUM=$(sha "$WORK/dl/master.ts")
PLAIN_SUM=$(sha "$WORK/hls/hi-plain.ts")
[ "$HLS_SUM" = "$PLAIN_SUM" ]; check $? "AES-128 decrypt matches reference plaintext"
$BIN --addr $ADDR limits --global 0 >/dev/null
check $? "global limiter cleared"

# ------------------------------------------- combo 3: HTTP x batch x torrent x magnet under gate
say "combo 3: concurrent HTTP x batch x torrent x magnet under max-active + global limit"
$BIN --addr $ADDR schedule --max-active 2 >/dev/null
$BIN --addr $ADDR limits --global 2MB >/dev/null
head -c 2097152 /dev/urandom > "$WORK/tdata.bin"
$BIN torrent-create "$WORK/tdata.bin" --announce "http://127.0.0.1:$TRACKER_PORT/announce" -o "$WORK/tdata.torrent" >/dev/null
[ -f "$WORK/tdata.torrent" ]; check $? "torrent created"
$BIN seed "$WORK/tdata.bin" --port $TRACKER_PORT >"$WORK/seed.log" 2>&1 &
SEED_PID=$!
sleep 1
MAGNET=$(grep -o 'magnet:?\S*' "$WORK/seed.log" | head -1)
[ -n "$MAGNET" ]; check $? "seeder up with magnet link"

# fire 4 tasks at once: torrent, magnet, single HTTP, batch of 3
TID=$($BIN --addr $ADDR torrent "$WORK/tdata.torrent" --dir "$WORK/dl" | grep -o '#[0-9]*' | tr -d '#')
MID=$($BIN --addr $ADDR magnet "$MAGNET" --dir "$WORK/dl/magnet" | grep -o '#[0-9]*' | tr -d '#')
BID=$($BIN --addr $ADDR add "http://127.0.0.1:$HTTP_PORT/xpic2.jpg" --dir "$WORK/dl" --conn 2 | grep -o '#[0-9]*' | tr -d '#')
$BIN --addr $ADDR batch "http://127.0.0.1:$HTTP_PORT/xpic[1:3].jpg" --dir "$WORK/dl" --conn 2 --limit 1MB >/dev/null

# poll the gate: never more than 2 active tasks at once
VIOL=0
for _ in $(seq 1 120); do
  N=$($BIN --addr $ADDR list --json | python3 -c '
import json,sys
a=json.load(sys.stdin)
print(sum(1 for t in a if t["state"] in ("downloading","probing","seeding","verifying")))' 2>/dev/null)
  [ "${N:-0}" -gt 2 ] && VIOL=$((VIOL+1))
  sleep 0.25
done
[ "$VIOL" = "0" ]; check $? "max-active=2 gate held for the whole mixed run (violations=$VIOL)"

wait_state "$TID" completed 180; check $? "torrent completed in mixed queue"
wait_state "$MID" completed 180; check $? "magnet completed in mixed queue"
wait_state "$BID" completed 120; check $? "HTTP download completed in mixed queue"
[ "$(sha "$WORK/dl/tdata.bin")" = "$(sha "$WORK/tdata.bin")" ]; check $? "torrent payload byte-identical"
[ "$(sha "$WORK/dl/magnet/tdata.bin")" = "$(sha "$WORK/tdata.bin")" ]; check $? "magnet payload byte-identical"
N=$(ls "$WORK/dl"/xpic*.jpg 2>/dev/null | wc -l)
[ "$N" = "3" ]; check $? "batch produced all 3 files alongside other traffic (got $N)"

# ------------------------------------------- combo 4: NMH x magnet (addTorrent) + NMH x HTTP
say "combo 4: NMH extension channel x magnet + HTTP"
NMH_OUT=$(python3 - "$NMH" "$ADDR" "$MAGNET" "$HTTP_PORT" <<'PYEOF'
import subprocess, struct, json, sys, os
nmh, addr, magnet, http_port = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
proc = subprocess.Popen([nmh], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, env={**os.environ, 'VELOCITYDM_ADDR': addr})
def send(o):
    b = json.dumps(o).encode()
    proc.stdin.write(struct.pack('<I', len(b)) + b); proc.stdin.flush()
def recv():
    n = struct.unpack('<I', proc.stdout.read(4))[0]
    return json.loads(proc.stdout.read(n))
send({"cmd": "ping"}); pong = recv()
send({"cmd": "addTorrent", "magnet": magnet}); at = recv()
send({"cmd": "add", "url": f"http://127.0.0.1:{http_port}/xpic1.jpg",
      "filename": "via-nmh.jpg"}); ad = recv()
send({"cmd": "status"}); st = recv()
proc.stdin.close(); proc.wait(timeout=10)
ok = pong.get("pong") and at.get("ok") and ad.get("ok") and st.get("ok")
print("OK" if ok else "FAIL", at.get("id", 0), ad.get("id", 0))
PYEOF
)
echo "$NMH_OUT" | grep -q '^OK'; check $? "NMH ping/addTorrent(magnet)/add/status roundtrip"
NTID=$(echo "$NMH_OUT" | awk '{print $2}')
NAID=$(echo "$NMH_OUT" | awk '{print $3}')
wait_state "${NTID:-999999}" completed 180; check $? "NMH-added magnet completed"
wait_state "${NAID:-999999}" completed 60; check $? "NMH-added HTTP download completed"
[ -f "$WORK/dl/via-nmh.jpg" ]; check $? "NMH download wrote the renamed file"

# ------------------------------------------- memory under mixed load
say "memory footprint (mixed load history)"
RSS=$(ps -o rss= -p $DAEMON_PID 2>/dev/null | tr -d ' ')
if [ -n "$RSS" ] && [ "$RSS" -lt 200000 ]; then
  ok "daemon RSS ${RSS}KB (< 200MB) after HTTP+HLS+BT+magnet+batch+NMH load"
else
  bad "daemon RSS ${RSS}KB"
fi

say "result"
printf '\n%d passed, %d failed\n' "$PASS" "$FAILS"
[ "$FAILS" = "0" ] && echo "ALL CROSS-FEATURE CHECKS PASSED" || echo "SOME CHECKS FAILED"
exit $FAILS
