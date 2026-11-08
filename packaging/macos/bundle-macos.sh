#!/usr/bin/env bash
# macOS .app bundle builder (run on macOS; the GUI binary must be built on a
# Mac because linking requires Apple's frameworks).
# Usage: ./bundle-macos.sh [release|debug]
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
SRC=$(cd "$HERE/../.." && pwd)
PROFILE=${1:-release}
APP="$SRC/dist/VelocityDM.app"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/MacOS/shared" \
         "$APP/Contents/Resources/extensions"

BIN="$SRC/target/$PROFILE/velocitydm"
NMH="$SRC/target/$PROFILE/velocitydm-nmh"
[ -x "$BIN" ] || { echo "missing $BIN (cargo build --$PROFILE)"; exit 1; }
[ -x "$NMH" ] || { echo "missing $NMH"; exit 1; }
cp "$BIN" "$APP/Contents/MacOS/velocitydm"
cp "$NMH" "$APP/Contents/MacOS/velocitydm-nmh"

cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>VelocityDM</string>
  <key>CFBundleDisplayName</key><string>VelocityDM</string>
  <key>CFBundleIdentifier</key><string>com.velocitydm.app</string>
  <key>CFBundleVersion</key><string>1.0.0</string>
  <key>CFBundleShortVersionString</key><string>1.0.0</string>
  <key>CFBundleExecutable</key><string>velocitydm</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeExtensions</key><array><string>torrent</string></array>
      <key>CFBundleTypeName</key><string>BitTorrent metainfo</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
    </dict>
  </array>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>Magnet URI</string>
      <key>CFBundleURLSchemes</key><array><string>magnet</string></array>
    </dict>
  </array>
</dict>
</plist>
EOF

# icon (reuse the generated 128 png as the app icon)
cp "$SRC/extensions/chromium/icons/icon128.png" "$APP/Contents/Resources/AppIcon.png"
cp "$SRC/extensions/chromium/icons/icon128.png" "$APP/Contents/Resources/icon128.png"

# browser extensions for easy load (unpacked)
cp -r "$SRC/extensions/dist/." "$APP/Contents/Resources/extensions/"

# native messaging host manifests
NMH_JSON="$APP/Contents/MacOS/com.velocitydm.nmh.json"
cat > "$NMH_JSON" <<EOF
{
  "name": "com.velocitydm.nmh",
  "description": "VelocityDM native messaging host",
  "path": "$APP/Contents/MacOS/velocitydm-nmh",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://REPLACE_WITH_EXTENSION_ID/"]
}
EOF
cat > "$APP/Contents/MacOS/com.velocitydm.nmh.firefox.json" <<EOF
{
  "name": "com.velocitydm.nmh",
  "description": "VelocityDM native messaging host",
  "path": "$APP/Contents/MacOS/velocitydm-nmh",
  "type": "stdio",
  "allowed_extensions": ["velocitydm@velocitydm.example"]
}
EOF

# install manifests for common browsers (best effort)
for d in "$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts" \
         "$HOME/Library/Application Support/Microsoft Edge/NativeMessagingHosts" \
         "$HOME/Library/Application Support/Chromium/NativeMessagingHosts"; do
  if [ -d "$(dirname "$d")" ]; then
    mkdir -p "$d"
    cp "$NMH_JSON" "$d/com.velocitydm.nmh.json"
    sed -i '' 's|REPLACE_WITH_EXTENSION_ID|REPLACE_WITH_EXTENSION_ID|' "$d/com.velocitydm.nmh.json"
  fi
done
FIREFOX_NM="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
if [ -d "$(dirname "$FIREFOX_NM")" ]; then
  mkdir -p "$FIREFOX_NM"
  cp "$APP/Contents/MacOS/com.velocitydm.nmh.firefox.json" "$FIREFOX_NM/com.velocitydm.nmh.json"
fi

echo "built $APP"
echo "Load About > Extensions (developer mode) from $APP/Contents/Resources/extensions/"
