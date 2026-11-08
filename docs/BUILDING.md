# Building, testing, packaging

## Prerequisites

- Rust 1.75+ (stable recommended): `https://rustup.rs`
- Node 18+ (extension tests only)
- Optional: Zig 0.13 + cargo-zigbuild (Windows cross-builds), Inno Setup
  (Windows installer), Android SDK (mobile app)

## Build

```sh
cargo build --release -p velocitydm -p velocitydm-nmh   # GUI + CLI + NMH
# headless-only build (no egui, for servers / Win7 payload):
cargo build --release -p velocitydm --no-default-features -p velocitydm-nmh
```

Binaries land in `target/release/`. The GUI binary embeds the engine; the
CLI talks to a daemon over 127.0.0.1:36990.

## Test

```sh
cargo test                      # all crates (122 vdm-core + 2 rpc tests + 14 engine-e2e)
node extensions/tests/run-tests.mjs      # 64 extension assertions
./scripts/e2e.sh                # 24-check cross-component smoke (release build)
./scripts/e2e-cross.sh          # 27-check feature-combination suite
./scripts/bench-perf.sh         # A/B perf benchmark (VDM_RTT_MS simulates WAN RTT)
```

The integration tests bind ephemeral localhost ports only.

## Cross-compiling

### Linux musl (static, old-distro baseline)

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release -p velocitydm --no-default-features --target x86_64-unknown-linux-musl
cargo build --release -p velocitydm-nmh --target x86_64-unknown-linux-musl
```

### Windows (needs mingw or Zig)

```sh
# with Zig (recommended from Linux):
cargo install cargo-zigbuild --locked
rustup target add x86_64-pc-windows-gnu
cargo zigbuild --release -p velocitydm --target x86_64-pc-windows-gnu
cargo zigbuild --release -p velocitydm-nmh --target x86_64-pc-windows-gnu
# x86:
rustup target add i686-pc-windows-gnu
cargo zigbuild --release -p velocitydm --target i686-pc-windows-gnu
```

`velocitydm.exe` is self-contained (no CRT installer needed with the gnu
toolchain). Then build the installer:

```sh
# wine iscc or on Windows:
iscc packaging/windows/velocitydm.iss
```

### Windows 7 payload

Windows 7 needs Rust 1.75 (last supported toolchain) and a headless build
(no egui/winit requirement there): build `velocitydm --no-default-features`
and `velocitydm-nmh` with the 1.75 toolchain; the browser extension popup
serves as the UI over NMH. If dependency crates demand newer editions, pin
older versions (edition-2021 compatible) — e.g. keep `serde < 1.0.204`,
`serde_json < 1.0.122`, `sha2 0.10`, `rustls 0.23` (all edition-2021).
A dedicated `win7/` workspace pinning these versions keeps this off the
main build.

### macOS

Cross-linking from Linux is not supported for eframe (needs Apple
frameworks). On a Mac:

```sh
cargo build --release -p velocitydm -p velocitydm-nmh
./packaging/macos/bundle-macos.sh
```

## Packaging

- Linux: `./packaging/linux/install.sh [--user|--system|--prefix DIR]`
  (binaries, `.desktop`, icons, NMH manifests) — also Flatpak:
  `packaging/flatpak/*.yml` (build binaries first, copy into `bin/`).
- Portable: copy `velocitydm`, `velocitydm-nmh` and
  `packaging/portable/velocitydm-portable.sh` into one folder.
- Windows: `packaging/windows/velocitydm.iss` + the payload binaries
  (registers NMH manifests for Chrome/Chromium/Mozilla per-user).
- Extensions: `./scripts/build-ext.sh` → `extensions/dist/*.zip`
  (Chrome/Edge/Opera/Firefox; Safari via `extensions/safari/README.md`).

### Native messaging host registration (Linux, per browser)

```sh
./packaging/nmh/install-nmh.sh --prefix /usr/local --extension-id <ID>
```

## Android

See `android/README.md` — Android Studio or
`./gradlew assembleRelease` with the SDK installed; ABI splits produce
armv7 (20001) and arm64 (20002) APKs.
