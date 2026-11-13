#!/usr/bin/env bash
# One-shot publisher for the reconciled tree: VelocityDM -> github.com/maniesh-NR/VelocityDM
#
# Designed for the REAL remote state (main advanced, tags + releases exist):
#   1. pushes main (fast-forward after the reconciliation merge)
#   2. force-moves the six tags to the new HEAD — ONE TAG PER PUSH COMMAND
#      (GitHub suppresses push events when >3 tags move at once), which
#      re-triggers the Release workflow per tag; the publish job attaches
#      fresh CI-built assets with `gh release upload --clobber`
#   3. verifies each tag actually moved on the remote
#   4. idempotent repo configuration (topics, security, labels, issues)
#
# Requires a token (in /home/z/my-project/.gh_token) with:
#   Metadata (R), Contents (RW), Administration (RW), Issues (RW),
#   Discussions (RW), Workflows (RW)
set -uo pipefail

TOK=$(cat /home/z/my-project/.gh_token)
REPO="maniesh-NR/VelocityDM"
API="https://api.github.com/repos/$REPO"
REPO_DIR="/home/z/my-project/velocitydm-repo"

api() { curl -s -H "Authorization: Bearer $TOK" -H "Accept: application/vnd.github+json" "$@"; }
gitpush() { # headless-safe auth: per-invocation Authorization header
  local tokb64
  tokb64=$(printf 'maniesh-NR:%s' "$TOK" | base64 -w0)
  git -c http.extraheader="Authorization: Basic $tokb64" push "$@"
}

cd "$REPO_DIR"
git remote set-url origin "https://github.com/$REPO.git" 2>/dev/null || git remote add origin "https://github.com/$REPO.git"

echo "== 1/4 push main =="
if gitpush origin main 2>&1 | tee /tmp/push-main.log; grep -q "rejected\|non-fast-forward" /tmp/push-main.log; then
  echo "PUSH FAILED - non-fast-forward (reconcile first: git fetch origin && git merge origin/main)"
  exit 1
fi
REMOTE_HEAD=$(git ls-remote origin refs/heads/main | cut -f1)
LOCAL_HEAD=$(git rev-parse HEAD)
[ "$REMOTE_HEAD" = "$LOCAL_HEAD" ] || { echo "PUSH FAILED - remote main ($REMOTE_HEAD) != local ($LOCAL_HEAD)"; exit 1; }
echo "main at $LOCAL_HEAD"

echo "== 2/4 re-cut the six tags at HEAD and force-move them (one per command so each push event fires) =="
TAGS=(v1.0.0 v1.0.0-linux v1.0.0-windows v1.0.0-macos v1.0.0-android v1.0.0-extensions)
for tag in "${TAGS[@]}"; do
  git tag -fa -m "VelocityDM 1.0.0 (${tag})" "$tag" "$LOCAL_HEAD" || { echo "RETAG FAILED: $tag"; exit 1; }
done
for tag in "${TAGS[@]}"; do
  gitpush origin -f "refs/tags/$tag:refs/tags/$tag" || { echo "TAG PUSH FAILED: $tag"; exit 1; }
  sleep 2
done

echo "== 3/4 verify tags moved =="
FAIL=0
for tag in "${TAGS[@]}"; do
  R=$(git ls-remote origin "refs/tags/$tag" | cut -f1)
  L=$(git rev-parse "refs/tags/$tag")
  if [ "$R" = "$L" ]; then echo "tag $tag -> $L OK"; else echo "tag $tag WRONG: remote=$R local=$L"; FAIL=1; fi
done
[ "$FAIL" = "0" ] || { echo "TAG VERIFY FAILED"; exit 1; }
echo "tags now point at the local tag objects for $LOCAL_HEAD"

echo "== 4/4 repository configuration (idempotent) =="
api -X PATCH "$API" -d '{
  "description": "Fast, clean-room download manager (IDM-style): segmented HTTP/HLS, BitTorrent + magnet, scheduler, browser integration. Rust engine + Material You UI for Windows/Linux/macOS/Android.",
  "homepage": "https://github.com/maniesh-NR/VelocityDM",
  "has_wiki": false,
  "has_discussions": true,
  "allow_squash_merge": true,
  "allow_merge_commit": false,
  "allow_rebase_merge": true,
  "delete_branch_on_merge": true
}' | jq -c '{full_name, description, has_discussions}' >/dev/null
api -X PUT -H "Content-Type: application/json" "$API/topics" -d '{"names":[
  "download-manager","idm-alternative","rust","egui","kotlin","cross-platform",
  "windows","windows-7","linux","macos","android","browser-extension",
  "chrome-extension","firefox-extension","edge-extension","opera-extension",
  "hls-downloader","bittorrent-client","magnet-links","multipart-download",
  "material-you","md3"
]}' -o /dev/null -w 'topics: %{http_code}\n'
api -X PUT "$API/vulnerability-alerts" -o /dev/null -w 'vuln-alerts: %{http_code}\n'
api -X PUT "$API/automated-security-fixes" -o /dev/null -w 'auto-sec-fixes: %{http_code}\n'

echo "== labels (create-if-missing) =="
for spec in \
  "area:engine|0e8a16|vdm-core engine (HTTP/HLS/BT/scheduler)" \
  "area:gui|c5def5|egui desktop GUI (Material You theme engine)" \
  "area:rpc|1d76db|JSON-RPC + SSE transport" \
  "area:nmh|b60205|native messaging host" \
  "area:extensions|5319e7|browser extensions" \
  "area:packaging|f9d0c4|installers, bundles, portable" \
  "area:ci|e99695|GitHub Actions workflows" \
  "area:harness|d4c5f9|agent harness, memory, backups" \
  "platform:windows|0078d7|Windows 10/11" \
  "platform:win7|a2eeef|Windows 7 payload" \
  "platform:linux|6cc644|Linux (glibc/musl)" \
  "platform:macos|8fbff7|macOS" \
  "platform:android|3ddc97|Android app" \
  "dependencies|0366d6|dependency updates (Dependabot)"; do
  IFS='|' read -r name color desc <<< "$spec"
  EXISTS=$(api "$API/labels/$name" | jq -r '.name // empty')
  if [ -z "$EXISTS" ]; then
    api -X POST "$API/labels" -d "{\"name\":\"$name\",\"color\":\"$color\",\"description\":\"$desc\"}" -o /dev/null
    echo "created $name"
  else
    api -X PATCH "$API/labels/$name" -d "{\"color\":\"$color\",\"description\":\"$desc\"}" -o /dev/null
    echo "updated $name"
  fi
done

echo "== milestone + roadmap issues (create-if-missing) =="
MID=$(api "$API/milestones?state=open" | jq -r '.[] | select(.title=="v1.1.0 - roadmap") | .number' | head -1)
if [ -z "$MID" ] || [ "$MID" = "null" ]; then
  MID=$(api -X POST "$API/milestones" -d '{"title":"v1.1.0 - roadmap","description":"Follow-ups after the 1.0.0 release (XDM / Motrix / Gopeed parity gaps).","state":"open"}' | jq -r '.number')
  echo "created milestone $MID"
else
  echo "milestone exists: $MID"
fi
issue() { # title body labels
  local t="$1" b="$2" l="$3"
  local found
  found=$(api "$API/issues?state=all&per_page=100" | jq -r --arg t "$t" '.[] | select(.title==$t) | .number' | head -1)
  if [ -n "$found" ] && [ "$found" != "null" ]; then
    echo "issue exists: #$found $t"
    return
  fi
  api -X POST "$API/issues" -d "$(jq -n --arg t "$t" --arg b "$b" --argjson m "$MID" --argjson l "$l" '{title:$t,body:$b,milestone:$m,labels:$l}')" | jq -r '.number'
}
issue "DHT support (BEP-5) so trackerless magnets resolve" "Private/dht-only magnets cannot resolve peers without a tracker today (documented in docs/REQUIREMENTS-COVERAGE.md). Implement BEP-5 + token-based announce." '["area:engine","bittorrent"]'
issue "UDP tracker support (BEP-15)" "UDP trackers are currently reported and skipped; magnets need http \`&tr=\` entries (see IMPROVEMENTS.md, from Motrix)." '["area:engine","bittorrent"]'
issue "DASH downloader: parse .mpd manifests and download segments" "DASH streams are detected and surfaced by the extension but not downloaded; HLS has a full engine, DASH does not (from XDM/Motrix parity)." '["area:engine","video"]'
issue "HLS: SAMPLE-AES and fMP4 EXT-X-MAP support" "Known limitation #4 in docs/REQUIREMENTS-COVERAGE.md." '["area:engine","video"]'
issue "Android: talk to a desktop daemon over LAN via the shared RPC" "The RPC protocol is shared; the Android app could control a desktop VelocityDM daemon on the LAN (noted in android/README.md, from Gopeed multi-protocol ideas)." '["platform:android","area:rpc"]'
issue "Safari native app extension" "Safari currently ships as a conversion guide (extensions/safari/README.md); build and document an Xcode-converted app extension." '["area:extensions"]'
issue "Publish on Flathub" "The Flatpak manifest lives in packaging/flatpak; submit and maintain the io.github.velocitydm.VelocityDM flathub package (from Gopeed distribution ideas)." '["area:packaging"]'

echo
echo "== done. The six Release workflows are now building per-platform assets;"
echo "   CI will clobber the release assets with fresh binaries from $LOCAL_HEAD."
echo "   Poll with: bash /home/z/my-project/scripts/poll-runs.sh 1800"
echo "   https://github.com/$REPO/actions"
