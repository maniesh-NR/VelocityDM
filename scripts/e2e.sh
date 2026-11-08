#!/usr/bin/env bash
# VelocityDM end-to-end integration smoke test.
# Exercises: daemon, CLI (http/batch/torrent/magnet/refresh/limits/schedule/
# settings), seeding, NMH frame protocol, SSE stream, checksums, memory.
set -u
cd "$(dirname "$0")/.."
BIN=./target/release/velocitydm
NMH=./target/release/velocitydm-nmh
ADDR=127.0.0.1:36991
WORK=$(mktemp -d /tmp/vdm-e2e-XXXX)
FAILS=0
PASS=0

say()  { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
ok()   { printf '\033[1;32mPASS\033[0m %s\n' "$*"; PASS=$((PASS+1)); }
bad()  { printf '\033[1;31mFAIL\033[0m %s\n' "$*"; FAILS=$((FAILS+1)); }
check(){ if [ "$1" = "0" ]; then ok "$2"; else bad "$2"; fi }

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

sha() { sha256sum "$1" | cut -d' ' -f1; }

say "test data"
mkdir -p "$WORK/files" "$WORK/dl"
head -c 3145728 /dev/urandom > "$WORK/files/big.bin"
for i in 1 2 3; do head -c 65536 /dev/urandom > "$WORK/files/pic$i.jpg"; done
python3 "$PWD/scripts/testserver.py" "$WORK/files" 8765 >"$WORK/http.log" 2>&1 &
HTTP_PID=$!
sleep 0.5

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

say "CLI: add segmented download with checksum"
SUM=$(sha "$WORK/files/big.bin")
ID=$($BIN --addr $ADDR add "http://127.0.0.1:8765/big.bin" --dir "$WORK/dl" --conn 8 --checksum "sha256:$SUM" | grep -o '#[0-9]*' | tr -d '#')
wait_state "$ID" completed 60
check $? "download reached completed state"
[ "$(sha "$WORK/dl/big.bin")" = "$SUM" ]
check $? "byte-identical file + sha256 checksum verified"
[ ! -f "$WORK/dl/big.bin.part" ] && [ ! -f "$WORK/dl/big.bin.vdmctl" ]
check $? "no leftover .part/.vdmctl after completion"

say "CLI: speed + progress observed"
SPEED=$($BIN --addr $ADDR list --json | python3 -c 'import json,sys;a=json.load(sys.stdin);print(1 if any(t["speed"]>0 for t in a) or all(t["state"]=="completed" for t in a) else 0)' 2>/dev/null)
[ "$SPEED" = "1" ]; check $? "list reports states/speeds"

say "CLI: batch pattern"
IDS=$($BIN --addr $ADDR batch "http://127.0.0.1:8765/pic[1:3].jpg" --dir "$WORK/dl")
sleep 3
N=$(ls "$WORK/dl"/pic*.jpg 2>/dev/null | wc -l)
[ "$N" = "3" ]; check $? "batch expanded to 3 files (got $N)"

say "CLI: refresh link on failed download"
DID=$($BIN --addr $ADDR add "http://127.0.0.1:8765/expired.bin" --dir "$WORK/dl" | grep -o '#[0-9]*' | tr -d '#')
wait_state "$DID" failed 60
check $? "dead link fails"
$BIN --addr $ADDR refresh "$DID" --url "http://127.0.0.1:8765/big.bin" >/dev/null
wait_state "$DID" completed 60
check $? "refresh link revives with new URL"
[ -f "$WORK/dl/expired.bin" ]; check $? "refresh kept original filename"

say "torrent create + seed + download"
head -c 2097152 /dev/urandom > "$WORK/tdata.bin"
$BIN torrent-create "$WORK/tdata.bin" --announce "http://127.0.0.1:36992/announce" -o "$WORK/tdata.torrent" >/dev/null
[ -f "$WORK/tdata.torrent" ]; check $? "torrent created"
$BIN seed "$WORK/tdata.bin" --port 36992 >"$WORK/seed.log" 2>&1 &
SEED_PID=$!
sleep 1
MAGNET=$(grep -o 'magnet:?\S*' "$WORK/seed.log" | head -1)
[ -n "$MAGNET" ]; check $? "seeder up with magnet link"
TID=$($BIN --addr $ADDR torrent "$WORK/tdata.torrent" --dir "$WORK/dl" | grep -o '#[0-9]*' | tr -d '#')
wait_state "$TID" completed 90
check $? "torrent download completed"
[ "$(sha "$WORK/dl/tdata.bin")" = "$(sha "$WORK/tdata.bin")" ]
check $? "torrent payload byte-identical"

say "magnet download"
MID=$($BIN --addr $ADDR magnet "$MAGNET" --dir "$WORK/dl/magnet" | grep -o '#[0-9]*' | tr -d '#')
wait_state "$MID" completed 90
check $? "magnet download completed"
[ "$(sha "$WORK/dl/magnet/tdata.bin")" = "$(sha "$WORK/tdata.bin")" ]
check $? "magnet payload byte-identical"

say "limits + scheduler + settings"
$BIN --addr $ADDR limits --global 2MB >/dev/null
L=$($BIN --addr $ADDR limits | grep -o '[0-9.]* MiB' | head -1)
[ -n "$L" ]; check $? "global limit set ($L)"
$BIN --addr $ADDR schedule --max-active 2 --stop-start 22:00 --stop-end 06:00 >/dev/null
Q=$($BIN --addr $ADDR schedule | grep -c '22:00')
[ "$Q" -ge 1 ]; check $? "scheduler stop window configured"
$BIN --addr $ADDR settings --dir "$WORK/dl" --conn 6 >/dev/null
S=$($BIN --addr $ADDR settings | python3 -c 'import json,sys;s=json.load(sys.stdin);print(s["defaultConnections"],s["downloadDir"])')
[ "$S" = "6 $WORK/dl" ]; check $? "settings persisted"

say "SSE event stream"
SSE=$(timeout 3 curl -sN "http://$ADDR/events" 2>/dev/null | head -c 4000)
echo "$SSE" | grep -q 'data: '; check $? "SSE stream emits data frames"
echo "$SSE" | grep -q 'task-added\|tasks'; check $? "SSE carries task events"

say "NMH frame protocol (add via extension channel)"
NMH_OUT=$(python3 - "$NMH" "$ADDR" <<'PYEOF'
import subprocess, struct, json, sys, time
nmh, addr = sys.argv[1], sys.argv[2]
# ensure no daemon auto-start interference: daemon already running
proc = subprocess.Popen([nmh], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, env={**__import__('os').environ, 'VELOCITYDM_ADDR': addr})
def send(o):
    b = json.dumps(o).encode()
    proc.stdin.write(struct.pack('<I', len(b)) + b); proc.stdin.flush()
def recv():
    n = struct.unpack('<I', proc.stdout.read(4))[0]
    return json.loads(proc.stdout.read(n))
send({"cmd": "ping"}); pong = recv()
send({"cmd": "add", "url": "http://127.0.0.1:8765/pic1.jpg",
      "filename": "from-extension.jpg", "referer": "http://127.0.0.1:8765/"})
added = recv()
send({"cmd": "status"}); status = recv()
proc.stdin.close(); proc.wait(timeout=10)
ok = pong.get("pong") and added.get("ok") and status.get("ok")
tid = added.get("id", 0)
print("OK" if ok else "FAIL", tid)
PYEOF
)
echo "$NMH_OUT" | grep -q '^OK'; check $? "NMH ping/add/status roundtrip"
EID=$(echo "$NMH_OUT" | awk '{print $2}')
wait_state "${EID:-999999}" completed 60
check $? "extension-added download completed"
[ -f "$WORK/dl/from-extension.jpg" ]; check $? "extension download wrote file"

say "memory footprint"
RSS=$(ps -o rss= -p $DAEMON_PID 2>/dev/null | tr -d ' ')
if [ -n "$RSS" ] && [ "$RSS" -lt 200000 ]; then
  ok "daemon RSS ${RSS}KB (< 200MB)"
else
  bad "daemon RSS ${RSS}KB"
fi

say "result"
printf '\n%d passed, %d failed\n' "$PASS" "$FAILS"
[ "$FAILS" = "0" ] && echo "ALL E2E CHECKS PASSED" || echo "SOME CHECKS FAILED"
exit $FAILS
