#!/usr/bin/env bash
# Build the browser extension packages from the chromium sources.
# Chrome/Edge/Opera/Firefox ship as zips (MV3 service worker; Firefox takes an
# event-page manifest variant). Safari builds an UNPACKED variant directory
# (S9, docs/SPECS/S9-SAFARI-XCODE.md rule 2): the Xcode conversion itself is
# witnessed on CI (see ci.yml "Safari app extension (Xcode conversion)");
# distribution is the owner Apple-ID lane (W2) — never a zip here.
set -eu
cd "$(dirname "$0")/.."
EXT=extensions
OUT="$EXT/dist"
rm -rf "$OUT"
mkdir -p "$OUT"

version=$(python3 -c 'import json;print(json.load(open("'$EXT'/chromium/manifest.json"))["version"])')

build_variant() { # name, prettyName, manifest-patcher
  local name=$1 pretty=$2
  local dir="$OUT/$name"
  rm -rf "$dir"
  cp -r "$EXT/chromium" "$dir"
  # background.js importScripts('shared/*.js') and popup/options pages load
  # shared/*.js — the shared modules must ship inside every package.
  rm -rf "$dir/shared" 2>/dev/null || true
  cp -r "$EXT/shared" "$dir/shared"
  rm -rf "$dir/icons" 2>/dev/null || true
  cp -r "$EXT/chromium/icons" "$dir/icons"
  python3 - "$dir/manifest.json" "$pretty" "$name" <<'PYEOF'
import json, sys
path, pretty, name = sys.argv[1], sys.argv[2], sys.argv[3]
m = json.load(open(path))
m['name'] = pretty
if name in ('firefox', 'safari'):
    # MV3 event page instead of a service worker: Firefox <109 service-worker
    # gaps and Safari's 15.4-16.3 importScripts breakage (Safari 16.4 release
    # note) both take the same manifest shape.
    m.pop('background', None)
    m['background'] = {
        'scripts': [
            'shared/detector.js', 'shared/patterns.js', 'shared/links.js',
            'shared/filters.js', 'shared/messaging.js', 'shared/nmhost.js',
            'shared/cookies.js', 'shared/takeover.js', 'shared/mime-table.js',
            'shared/proxyreport.js', 'shared/state.js', 'background.js'
        ]
    }
    m.pop('minimum_chrome_version', None)
    # Event-page backgrounds must not call importScripts (Firefox never
    # supported it there; Safari 15.4-16.3 broke it in service workers).
    bg = open(path.replace('manifest.json', 'background.js')).read()
    bg = bg.replace(
        "importScripts(\n  'shared/detector.js',\n  'shared/patterns.js',\n  'shared/links.js',\n  'shared/filters.js',\n  'shared/messaging.js',\n  'shared/nmhost.js',\n  'shared/cookies.js',\n  'shared/takeover.js',\n  'shared/mime-table.js',\n  'shared/proxyreport.js',\n  'shared/state.js'\n);",
        "// Event page: shared modules are loaded via manifest scripts")
    open(path.replace('manifest.json', 'background.js'), 'w').write(bg)
if name == 'firefox':
    m['browser_specific_settings'] = {
        'gecko': {'id': 'velocitydm@velocitydm.example', 'strict_min_version': '109.0'}
    }
if name == 'safari':
    # S9 (docs/SPECS/S9-SAFARI-XCODE.md rule 2 + the gate capability matrix):
    # Safari has no chrome.downloads (MDN BCD: version_added false) and
    # chrome.proxy is unverified (the Safari 17 note covers WKWebView only).
    # Both drop from the manifest; the background's feature guards (assessment
    # 0019 C2a) degrade the intercept categories by omission. No gecko id, no
    # minimum_chrome_version; the version stays the frozen chromium one.
    m['permissions'] = [p for p in m['permissions'] if p not in ('downloads', 'proxy')]
json.dump(m, open(path, 'w'), indent=2)
PYEOF
  if [ "$name" = "safari" ]; then
    # Never a zip: the release glob extensions/dist/*.zip and the explicit
    # release.yml publish mapping must both stay unable to pick this up.
    echo "built $OUT/$name (unpacked dir only; conversion witnessed on CI)"
  else
    (cd "$OUT" && zip -qr "$name-$version.zip" "$name")
    echo "built $OUT/$name-$version.zip"
  fi
}

build_variant chromium "VelocityDM Download Manager"
build_variant edge "VelocityDM Download Manager"
build_variant opera "VelocityDM Download Manager"
build_variant firefox "VelocityDM Download Manager"
build_variant safari "VelocityDM Download Manager"

ls -la "$OUT"
ls "$OUT"/*.zip
