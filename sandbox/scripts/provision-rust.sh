#!/usr/bin/env bash
# Provision Rust toolchains from static.rust-lang.org dist tarballs.
# (rustup-init dies silently in this sandbox; this is the proven recipe —
#  session 20, MEMORY.md. Idempotent: skips components already present.)
#
# Usage: provision-rust.sh [stable-version] [witness-version]
#   default: 1.100.0 1.75.0
set -euo pipefail

STABLE_VER="${1:-1.100.0}"
WITNESS_VER="${2:-1.75.0}"
RUSTUP_HOME=/home/z/my-project/tools/rustup
DIST=/tmp/rust-dist
TGT=x86_64-unknown-linux-gnu

stable_tc="$RUSTUP_HOME/toolchains/stable-$TGT"
witness_tc="$RUSTUP_HOME/toolchains/$WITNESS_VER-$TGT"
shims="$HOME/.cargo/bin"

mkdir -p "$DIST"

fetch_channel() { # version -> prints component url map on stdout
  local ver="$1"
  curl -fsSL "https://static.rust-lang.org/dist/channel-rust-$ver.toml" -o "$DIST/channel-$ver.toml"
  python3 - "$DIST/channel-$ver.toml" "$TGT" <<'PY'
import sys, tomllib
path, tgt = sys.argv[1], sys.argv[2]
with open(path, 'rb') as f:
    ch = tomllib.load(f)
for comp in ('rustc', 'rust-std', 'cargo', 'rustfmt-preview', 'clippy-preview'):
    pkg = ch['pkg'].get(comp)
    if not pkg:
        continue
    t = pkg['target'].get(tgt)
    if not t:
        continue
    print(f"{comp} {t['url']} {t['hash']}")
PY
}

install_component() { # url sha dest_toolchain
  local url="$1" sha="$2" dest="$3"
  local base="$(basename "$url")"; local fn="$DIST/$base"
  if [ ! -f "$fn" ]; then
    echo "download $base"
    curl -fsSL "$url" -o "$fn"
  fi
  echo "$sha  $fn" | sha256sum -c - >/dev/null || { echo "SHA MISMATCH $fn"; exit 1; }
  local xdir="$DIST/x-${base%.tar.*}"
  rm -rf "$xdir"; mkdir -p "$xdir"
  tar -xf "$fn" -C "$xdir"
  # merge component dirs (xdir/<package>/<component>/) into dest
  local merged=0
  for compdir in "$xdir"/*/*/; do
    [ -d "$compdir" ] || continue
    (cd "$compdir" && tar cf - .) | (cd "$dest" && tar xf -)
    merged=1
  done
  [ "$merged" = 1 ] || { echo "no component dir found in $fn"; exit 1; }
  rm -rf "$xdir"
}

need() { # file ver
  [ -x "$1" ] || return 0
  return 1
}

# --- stable ---
if [ ! -x "$stable_tc/bin/rustc" ] || [ ! -x "$stable_tc/bin/cargo" ]; then
  echo "== provisioning stable $STABLE_VER =="
  rm -rf "$stable_tc"; mkdir -p "$stable_tc"
  fetch_channel "$STABLE_VER" | while read -r comp url sha; do
    install_component "$url" "$sha" "$stable_tc"
  done
else
  echo "stable $STABLE_VER already provisioned"
fi
# clippy/rustfmt completeness (survives partial gutting)
for b in rustfmt cargo-fmt; do
  [ -x "$stable_tc/bin/$b" ] || { echo "missing $b — reprovision"; exit 1; }
done

# --- witness (1.75.0) ---
if [ ! -x "$witness_tc/bin/rustc" ] || [ ! -x "$witness_tc/bin/cargo" ]; then
  echo "== provisioning witness $WITNESS_VER =="
  rm -rf "$witness_tc"; mkdir -p "$witness_tc"
  fetch_channel "$WITNESS_VER" | while read -r comp url sha; do
    case "$comp" in rustc|rust-std|cargo) install_component "$url" "$sha" "$witness_tc";; esac
  done
else
  echo "witness $WITNESS_VER already provisioned"
fi

# --- shims ---
mkdir -p "$shims"
for b in cargo rustc rustfmt cargo-fmt cargo-clippy clippy-driver; do
  [ -x "$stable_tc/bin/$b" ] && ln -sf "$stable_tc/bin/$b" "$shims/$b"
done

export RUSTUP_HOME CARGO_HOME=/home/z/my-project/tools/cargo
echo "== verify =="
"$shims/cargo" --version
"$shims/rustc" --version
"$shims/cargo" fmt --version
"$shims/cargo" clippy --version
PATH="$witness_tc/bin:$PATH" cargo --version
echo OK
