#!/usr/bin/env bash
# VelocityDM macOS .app assembler (SPDX-License-Identifier: GPL-3.0-or-later).
# Run on macOS after: cargo build --release --workspace
set -eu
APP=dist/VelocityDM.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/velocitydm "$APP/Contents/MacOS/"
cp target/release/velocitydm-nmh "$APP/Contents/MacOS/"
cat > "$APP/Contents/Info.plist" << 'PLISTEOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>VelocityDM</string>
  <key>CFBundleIdentifier</key><string>local.velocitydm.VelocityDM</string>
  <key>CFBundleVersion</key><string>1.0.0</string>
  <key>CFBundleExecutable</key><string>velocitydm</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLISTEOF
cat > "$APP/Contents/Resources/com.velocitydm.nmh.json" << 'NMHEOF'
{
  "name": "com.velocitydm.nmh",
  "description": "VelocityDM native messaging host",
  "path": "/Applications/VelocityDM.app/Contents/MacOS/velocitydm-nmh",
  "type": "stdio",
  "allowed_extensions": ["velocitydm@velocitydm.local"]
}
NMHEOF
# User-level NMH registration for Chromium browsers on macOS.
for dir in \
  "$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts" \
  "$HOME/Library/Application Support/Chromium/NativeMessagingHosts" \
  "$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"; do
  mkdir -p "$dir"
  cp "$APP/Contents/Resources/com.velocitydm.nmh.json" "$dir/com.velocitydm.nmh.json"
done
echo "Built $APP - move it to /Applications and register the NMH manifest if needed."
