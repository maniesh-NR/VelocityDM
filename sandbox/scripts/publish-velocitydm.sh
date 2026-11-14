#!/usr/bin/env bash
# One-shot publisher: VelocityDM -> github.com/maniesh-NR/VelocityDM
# Pushes main + tags (tags trigger the CI release builds), configures the
# repository (topics, security, discussions), creates the six releases with
# prebuilt/verified assets as a floor while CI adds fresh per-platform builds,
# and sets up labels, milestone and roadmap issues.
#
# Requires a token (in /home/z/my-project/.gh_token) with:
#   Metadata (R), Contents (RW), Administration (RW), Issues (RW),
#   Discussions (RW), Workflows (RW)
set -uo pipefail

TOK=$(cat /home/z/my-project/.gh_token)
REPO="maniesh-NR/VelocityDM"
API="https://api.github.com/repos/$REPO"
REPO_DIR="/home/z/my-project/velocitydm-repo"
ASSETS="/home/z/my-project/release-assets"
NOTES_DIR="/home/z/my-project/release-notes"
mkdir -p "$NOTES_DIR"

api() { curl -s -H "Authorization: Bearer $TOK" -H "Accept: application/vnd.github+json" "$@"; }
gitpush() { # headless-safe auth: per-invocation Authorization header
  local tokb64
  tokb64=$(printf 'maniesh-NR:%s' "$TOK" | base64 -w0)
  git -c http.extraheader="Authorization: Basic $tokb64" push "$@"
}

cd "$REPO_DIR"
git remote set-url origin "https://github.com/$REPO.git" 2>/dev/null || git remote add origin "https://github.com/$REPO.git"

echo "== 1/6 push main =="
gitpush -u origin main || { echo "PUSH FAILED - token still lacks Contents:write"; exit 1; }

echo "== 2/6 push tags (triggers release builds via .github/workflows/release.yml) =="
gitpush origin --tags

echo "== 3/6 repository configuration =="
api -X PATCH "$API" -d '{
  "description": "Fast, clean-room download manager (IDM-style): segmented HTTP/HLS, BitTorrent + magnet, scheduler, browser integration. Rust engine + MD3 egui GUI for Windows/Linux/macOS/Android.",
  "homepage": "https://github.com/maniesh-NR/VelocityDM",
  "has_wiki": false,
  "has_discussions": true,
  "allow_squash_merge": true,
  "allow_merge_commit": false,
  "allow_rebase_merge": true,
  "delete_branch_on_merge": true
}' | jq -c '{full_name, private, description, has_discussions}'
api -X PUT -H "Content-Type: application/json" "$API/topics" -d '{"names":[
  "download-manager","idm-alternative","rust","egui","kotlin","cross-platform",
  "windows","windows-7","linux","macos","android","browser-extension",
  "chrome-extension","firefox-extension","edge-extension","opera-extension",
  "hls-downloader","bittorrent-client","magnet-links","multipart-download"
]}' | jq -c .
# security hardening (best effort)
api -X PUT "$API/vulnerability-alerts" -o /dev/null -w 'vuln-alerts: %{http_code}\n'
api -X PUT "$API/automated-security-fixes" -o /dev/null -w 'auto-sec-fixes: %{http_code}\n'
api -X POST "$API/private-vulnerability-reporting" -o /dev/null -w 'private-vuln-reporting: %{http_code}\n'
api -X PATCH "$API" -d '{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"}}}' -o /dev/null -w 'secret-scanning: %{http_code}\n'

echo "== 4/6 labels =="
label() { api -X POST "$API/labels" -d "{\"name\":\"$1\",\"color\":\"$2\",\"description\":\"$3\"}" | jq -r '.name // .message' ; }
label "area:engine"        "0e8a16" "vdm-core engine (HTTP/HLS/BT/scheduler)"
label "area:gui"           "c5def5" "egui desktop GUI"
label "area:rpc"           "1d76db" "JSON-RPC + SSE transport"
label "area:nmh"           "b60205" "native messaging host"
label "area:extensions"    "5319e7" "browser extensions"
label "area:packaging"     "f9d0c4" "installers, bundles, portable"
label "area:ci"            "e99695" "GitHub Actions workflows"
label "platform:windows"   "0078d7" "Windows 10/11"
label "platform:win7"      "a2eeef" "Windows 7 payload"
label "platform:linux"     "6cc644" "Linux (glibc/musl)"
label "platform:macos"     "8fbff7" "macOS"
label "platform:android"   "3ddc97" "Android app"
label "dependencies"       "0366d6" "dependency updates (Dependabot)"

echo "== 5/6 releases (prebuilt verified assets; CI adds fresh per-platform builds) =="
notes_for() {
  local tag=$1 plat=$2
  ( cd "$REPO_DIR" && TAG="$tag" VER="1.0.0" PLATFORM="$plat" bash .github/workflows/release-notes.sh >/dev/null && mv notes.md "$NOTES_DIR/$tag.md" )
}
release() { # tag platform assets...
  local tag=$1 plat=$2; shift 2
  notes_for "$tag" "$plat"
  local latest="false"; [ "$plat" = "all" ] && latest="true"
  local body; body=$(jq -Rs . < "$NOTES_DIR/$tag.md")
  api -X POST "$API/releases" -d "{\"tag_name\":\"$tag\",\"name\":\"VelocityDM $tag\",\"body\":$body,\"prerelease\":false,\"make_latest\":\"$latest\"}" | jq -r '.id' > /tmp/relid
  local rid; rid=$(cat /tmp/relid)
  if [ "$rid" != "null" ] && [ -n "$rid" ]; then
    ( cd "$ASSETS" && for f in "$@"; do
        [ -f "$f" ] || continue
        sha256sum "$f" >> SHA256SUMS-$tag.txt
      done
      [ -f SHA256SUMS-$tag.txt ] && curl -s -X POST -H "Authorization: Bearer $TOK" -H "Content-Type: text/plain" --data-binary @SHA256SUMS-$tag.txt "https://uploads.github.com/repos/$REPO/releases/$rid/assets?name=SHA256SUMS.txt" -o /dev/null
      for f in "$@"; do
        [ -f "$f" ] || continue
        curl -s -X POST -H "Authorization: Bearer $TOK" -H "Content-Type: application/octet-stream" --data-binary @"$f" "https://uploads.github.com/repos/$REPO/releases/$rid/assets?name=$f" -o /dev/null -w "upload $f: %{http_code}\n"
      done )
  else
    echo "release $tag: create failed (may already exist)"
  fi
}
release v1.0.0            all        velocitydm-1.0.0-linux-x64.tar.gz velocitydm-1.0.0-linux-x64-musl.tar.gz velocitydm-1.0.0-windows-x64.zip velocitydm-1.0.0-windows-x86.zip velocitydm-1.0.0-windows7-x64.zip velocitydm-1.0.0-windows7-x86.zip chromium-1.0.0.zip edge-1.0.0.zip firefox-1.0.0.zip opera-1.0.0.zip
release v1.0.0-linux      linux      velocitydm-1.0.0-linux-x64.tar.gz velocitydm-1.0.0-linux-x64-musl.tar.gz
release v1.0.0-windows    windows    velocitydm-1.0.0-windows-x64.zip velocitydm-1.0.0-windows-x86.zip velocitydm-1.0.0-windows7-x64.zip velocitydm-1.0.0-windows7-x86.zip
release v1.0.0-extensions extensions chromium-1.0.0.zip edge-1.0.0.zip firefox-1.0.0.zip opera-1.0.0.zip
release v1.0.0-macos      macos
release v1.0.0-android    android

echo "== 6/6 milestone + roadmap issues =="
MID=$(api -X POST "$API/milestones" -d '{"title":"v1.1.0 - roadmap","description":"Follow-ups after the 1.0.0 release (XDM / Motrix / Gopeed parity gaps).","state":"open"}' | jq -r '.number')
echo "milestone: $MID"
issue() { api -X POST "$API/issues" -d "$(jq -n --arg t "$1" --arg b "$2" --argjson m "$MID" --argjson l "$3" '{title:$t,body:$b,milestone:$m,labels:$l}')" | jq -r '.number'; }
issue "DHT support (BEP-5) so trackerless magnets resolve" "Private/dht-only magnets cannot resolve peers without a tracker today (documented in docs/REQUIREMENTS-COVERAGE.md). Implement BEP-5 + token-based announce." '["area:engine","bittorrent"]'
issue "UDP tracker support (BEP-15)" "UDP trackers are currently reported and skipped; magnets need http \`&tr=\` entries (see IMPROVEMENTS.md, from Motrix)." '["area:engine","bittorrent"]'
issue "DASH downloader: parse .mpd manifests and download segments" "DASH streams are detected and surfaced by the extension but not downloaded; HLS has a full engine, DASH does not (from XDM/Motrix parity)." '["area:engine","video"]'
issue "HLS: SAMPLE-AES and fMP4 EXT-X-MAP support" "Known limitation #4 in docs/REQUIREMENTS-COVERAGE.md." '["area:engine","video"]'
issue "Android: talk to a desktop daemon over LAN via the shared RPC" "The RPC protocol is shared; the Android app could control a desktop VelocityDM daemon on the LAN (noted in android/README.md, from Gopeed multi-protocol ideas)." '["platform:android","area:rpc"]'
issue "Safari native app extension" "Safari currently ships as a conversion guide (extensions/safari/README.md); build and document an Xcode-converted app extension." '["area:extensions"]'
issue "Publish on Flathub" "The Flatpak manifest lives in packaging/flatpak; submit and maintain the io.github.velocitydm.VelocityDM flathub package (from Gopeed distribution ideas)." '["area:packaging"]'

echo
echo "== done. CI is now building per-platform release assets; check:"
echo "   https://github.com/$REPO/actions"
