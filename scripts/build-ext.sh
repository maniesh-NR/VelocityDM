#!/usr/bin/env bash
# Build the four browser extension packages from the chromium sources.
# Chrome/Edge/Opera are MV3 service-worker builds; Firefox uses an event-page
# manifest variant. Safari: see extensions/safari/README.md for conversion.
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
if name == 'firefox':
    # Firefox MV3: event page instead of a service worker
    m.pop('background', None)
    m['background'] = {
        'scripts': [
            'shared/detector.js', 'shared/patterns.js', 'shared/links.js',
            'shared/filters.js', 'shared/messaging.js', 'shared/state.js',
            'background.js'
        ]
    }
    m['browser_specific_settings'] = {
        'gecko': {'id': 'velocitydm@velocitydm.example', 'strict_min_version': '109.0'}
    }
    m.pop('minimum_chrome_version', None)
    # Firefox background.js must not call importScripts
    bg = open(path.replace('manifest.json', 'background.js')).read()
    bg = bg.replace(
        "importScripts(\n  'shared/detector.js',\n  'shared/patterns.js',\n  'shared/links.js',\n  'shared/filters.js',\n  'shared/messaging.js',\n  'shared/state.js'\n);",
        "// Firefox event page: shared modules are loaded via manifest scripts")
    open(path.replace('manifest.json', 'background.js'), 'w').write(bg)
json.dump(m, open(path, 'w'), indent=2)
PYEOF
  (cd "$OUT" && zip -qr "$name-$version.zip" "$name")
  echo "built $OUT/$name-$version.zip"
}

build_variant chromium "VelocityDM Download Manager"
build_variant edge "VelocityDM Download Manager"
build_variant opera "VelocityDM Download Manager"
build_variant firefox "VelocityDM Download Manager"

ls -la "$OUT"/*.zip
