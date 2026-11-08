#!/usr/bin/env bash
# Install the VelocityDM native messaging host manifests for the browsers
# found on this Linux system.
#
# Usage:
#   ./install-nmh.sh [--prefix DIR] [--extension-id ID] [--uninstall]
#
# --prefix DIR        where velocitydm binaries live (default: alongside
#                     this script's parent, i.e. the install prefix)
# --extension-id ID   the Chrome/Edge/Opera extension id shown after loading
#                     the unpacked extension (needed for Chromium-family
#                     browsers; Firefox uses its fixed add-on id).
set -eu

PREFIX=""
EXT_ID=""
UNINSTALL=0
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) PREFIX="$2"; shift 2 ;;
    --extension-id) EXT_ID="$2"; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

if [ -z "$PREFIX" ]; then
  HERE=$(cd "$(dirname "$0")/.." && pwd)
  PREFIX="$HERE"
fi
NMH_BIN="$PREFIX/bin/velocitydm-nmh"
[ -x "$NMH_BIN" ] || NMH_BIN="$PREFIX/velocitydm-nmh"
if [ $UNINSTALL -eq 0 ] && [ ! -x "$NMH_BIN" ]; then
  echo "velocitydm-nmh not found in $PREFIX (use --prefix)" >&2
  exit 1
fi

HOST_NAME=com.velocitydm.nmh
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)

CHROME_DIRS=(
  "$HOME/.config/google-chrome/NativeMessagingHosts"
  "$HOME/.config/chromium/NativeMessagingHosts"
  "$HOME/.config/microsoft-edge/NativeMessagingHosts"
  "$HOME/.config/opera/NativeMessagingHosts"
  "$HOME/.config/vivaldi/NativeMessagingHosts"
  "$HOME/.config/brave/NativeMessagingHosts"
)
FIREFOX_DIR="$HOME/.mozilla/native-messaging-hosts"

if [ $UNINSTALL -eq 1 ]; then
  for d in "${CHROME_DIRS[@]}" "$FIREFOX_DIR"; do
    rm -f "$d/$HOST_NAME.json"
  done
  echo "native messaging host uninstalled"
  exit 0
fi

# Chromium family
INSTALLED=0
for d in "${CHROME_DIRS[@]}"; do
  if [ -d "$(dirname "$d")" ]; then
    mkdir -p "$d"
    if [ -n "$EXT_ID" ]; then
      sed -e "s|@NMH_BINARY_PATH@|$NMH_BIN|" -e "s|@EXTENSION_ID@|$EXT_ID|" \
        "$SCRIPT_DIR/$HOST_NAME.json" > "$d/$HOST_NAME.json"
      echo "installed for $(basename "$(dirname "$d")") (extension $EXT_ID)"
      INSTALLED=1
    else
      echo "note: $(basename "$(dirname "$d")") present but no --extension-id given; manifest skipped" >&2
    fi
  fi
done

# Firefox (fixed add-on id)
if [ -d "$HOME/.mozilla" ]; then
  mkdir -p "$FIREFOX_DIR"
  sed -e "s|@NMH_BINARY_PATH@|$NMH_BIN|" \
    "$SCRIPT_DIR/$HOST_NAME.firefox.json" > "$FIREFOX_DIR/$HOST_NAME.json"
  echo "installed for Firefox"
  INSTALLED=1
fi

if [ $INSTALLED -eq 0 ]; then
  echo "no browsers detected; nothing installed" >&2
  exit 1
fi
echo "done. Restart your browser, then load the extension and check Settings > Test connection."
