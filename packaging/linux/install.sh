#!/usr/bin/env bash
# Linux installer for VelocityDM (system or user install).
# Usage: ./install.sh [--user|--prefix DIR] [--uninstall]
set -eu

MODE="--user"
PREFIX=""
UNINSTALL=0
while [ $# -gt 0 ]; do
  case "$1" in
    --user) MODE="--user"; shift ;;
    --system) MODE="--system"; shift ;;
    --prefix) MODE="--prefix"; PREFIX="$2"; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    *) echo "usage: $0 [--user|--system|--prefix DIR] [--uninstall]" >&2; exit 2 ;;
  esac
done

HERE=$(cd "$(dirname "$0")" && pwd)
SRC=$(cd "$HERE/../.." && pwd)

if [ "$MODE" = "--user" ]; then
  PREFIX="$HOME/.local"
elif [ "$MODE" = "--system" ]; then
  PREFIX="/usr/local"
fi
BINDIR="$PREFIX/bin"
if [ -n "$(command -v systemctl)" ] && [ "$PREFIX" = "/usr/local" ]; then
  : # system install shares the same layout
fi

if [ $UNINSTALL -eq 1 ]; then
  rm -f "$BINDIR/velocitydm" "$BINDIR/velocitydm-nmh"
  rm -f "$PREFIX/share/applications/velocitydm.desktop"
  rm -f "$PREFIX/share/icons/hicolor/128x128/apps/velocitydm.png" \
        "$PREFIX/share/icons/hicolor/48x48/apps/velocitydm.png" \
        "$PREFIX/share/icons/hicolor/16x16/apps/velocitydm.png"
  for d in "$HOME/.config/google-chrome/NativeMessagingHosts" \
           "$HOME/.config/chromium/NativeMessagingHosts" \
           "$HOME/.config/microsoft-edge/NativeMessagingHosts" \
           "$HOME/.config/opera/NativeMessagingHosts" \
           "$HOME/.mozilla/native-messaging-hosts"; do
    rm -f "$d/com.velocitydm.nmh.json"
  done
  update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
  echo "VelocityDM uninstalled"
  exit 0
fi

GUI_BIN="$SRC/target/release/velocitydm"
NMH_BIN="$SRC/target/release/velocitydm-nmh"
[ -x "$GUI_BIN" ] || GUI_BIN="$SRC/velocitydm"
[ -x "$NMH_BIN" ] || NMH_BIN="$SRC/velocitydm-nmh"
if [ ! -x "$GUI_BIN" ] || [ ! -x "$NMH_BIN" ]; then
  echo "binaries not found; build first (cargo build --release)" >&2
  exit 1
fi

mkdir -p "$BINDIR" "$PREFIX/share/applications" "$PREFIX/share/icons/hicolor/128x128/apps" \
         "$PREFIX/share/icons/hicolor/48x48/apps" "$PREFIX/share/icons/hicolor/16x16/apps"
install -m 755 "$GUI_BIN" "$BINDIR/velocitydm"
install -m 755 "$NMH_BIN" "$BINDIR/velocitydm-nmh"
install -m 644 "$HERE/velocitydm.desktop" "$PREFIX/share/applications/velocitydm.desktop"
install -m 644 "$SRC/extensions/chromium/icons/icon128.png" "$PREFIX/share/icons/hicolor/128x128/apps/velocitydm.png"
install -m 644 "$SRC/extensions/chromium/icons/icon48.png" "$PREFIX/share/icons/hicolor/48x48/apps/velocitydm.png"
install -m 644 "$SRC/extensions/chromium/icons/icon16.png" "$PREFIX/share/icons/hicolor/16x16/apps/velocitydm.png"
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
echo "installed to $PREFIX"

# native messaging host (Firefox is deterministic; Chromium needs the id)
"$HERE/../nmh/install-nmh.sh" --prefix "$PREFIX" || true
echo
echo "Next: load extensions/dist/<browser>-1.0.0.zip in your browser,"
echo "note its extension id, and re-run:"
echo "  $HERE/../nmh/install-nmh.sh --prefix $PREFIX --extension-id <ID>"
