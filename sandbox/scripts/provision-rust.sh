#!/usr/bin/env bash
# Rebuild gutted rustup toolchains from dist tarballs (no rustup-init needed).
# Usage: bash provision-rust.sh
set -euo pipefail
RUSTUP=/home/z/my-project/tools/rustup
STABLE=$RUSTUP/toolchains/stable-x86_64-unknown-linux-gnu
OLD175=$RUSTUP/toolchains/1.75.0-x86_64-unknown-linux-gnu
WORK=$(mktemp -d /tmp/rustprov.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

fetch() { # url dest
  echo "GET $1"
  curl -sSfL --retry 3 --max-time 300 -o "$2" "$1"
}

merge() { # tarball
  local tb="$1"
  tar -xzf "$tb" -C "$WORK"
}

# --- stable 1.100.0: rustc + std + cargo + rustfmt ---
STABLE_VER=1.100.0
for pkg in rustc rust-std cargo rustfmt; do
  fetch "https://static.rust-lang.org/dist/${pkg}-${STABLE_VER}-x86_64-unknown-linux-gnu.tar.gz" \
        "$WORK/${pkg}.tar.gz"
  merge "$WORK/${pkg}.tar.gz"
done
for pkg in rustc rust-std cargo rustfmt; do
  src="$WORK/${pkg}-${STABLE_VER}-x86_64-unknown-linux-gnu"
  # each dist package unpacks component dirs (e.g. rustc/bin, rustc/lib,
  # rust-std-*/rust-std/lib/rustlib, cargo/bin, cargo/lib)
  find "$src" -mindepth 1 -maxdepth 1 -type d | while read -r comp; do
    cp -a "$comp/." "$STABLE/"
  done
done
chmod +x "$STABLE/bin/"* || true

# --- 1.75.0: rustc + std + cargo ---
for pkg in rustc rust-std cargo; do
  fetch "https://static.rust-lang.org/dist/${pkg}-1.75.0-x86_64-unknown-linux-gnu.tar.gz" \
        "$WORK/175-${pkg}.tar.gz"
  merge "$WORK/175-${pkg}.tar.gz"
done
for pkg in rustc rust-std cargo; do
  src="$WORK/${pkg}-1.75.0-x86_64-unknown-linux-gnu"
  find "$src" -mindepth 1 -maxdepth 1 -type d | while read -r comp; do
    cp -a "$comp/." "$OLD175/"
  done
done
chmod +x "$OLD175/bin/"* || true

# --- shims in ~/.cargo/bin ---
mkdir -p "$HOME/.cargo/bin"
cd "$HOME/.cargo/bin"
for t in rustc cargo rustdoc; do ln -sf "$STABLE/bin/$t" "$t"; done
for t in rustfmt cargo-fmt; do ln -sf "$STABLE/bin/$t" "$t"; done
for t in cargo-clippy clippy-driver; do ln -sf "$STABLE/bin/$t" "$t"; done
echo "PROVISION DONE"
