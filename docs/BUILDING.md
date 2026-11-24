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

The integration tests bind ephemeral localhost ports only. The DHT (S4)
tests use an in-process mini-DHT over loopback UDP — the public DHT is never
touched by the test suite; `VDM_DHT_BOOTSTRAP=off` keeps the e2e harnesses
offline as well.

**DHT privacy note (S4):** DHT lookups inherently reveal the info_hash to
the queried nodes — that is BEP-5's design. Nothing beyond the info_hash,
our node id, and a UDP port leaves the process (no names, no paths, no
credentials); announce tokens are never logged. The public BEP-5 routers
are resolved by DNS at runtime and queried once per download; set
`VDM_DHT_BOOTSTRAP=off` to disable the DHT bootstrap entirely, or a
comma-separated `host:port` list to pin custom routers.

**Connection presets note (S11-i B2p):** the `connPreset` option
(`auto|high|medium|low`, default `auto`) caps the BASE connection count per
link class — a ceiling, never a raise (`high`=8, `medium`=4, `low`=2).
Honest warning: high connection counts can antagonize servers (the
reference product itself warns about this) — presets default to `auto`.

**TLS minimum-version note (S11-i B22):** the `tlsMin` setting
(`default|tls12|tls13`, default `default`) raises the TLS floor for engine
HTTP(S) downloads — `default` keeps rustls's safe defaults (TLS 1.2+1.3,
zero change); rustls+ring supports nothing below TLS 1.2, so the knob is
floor-raising by construction. Torrent tracker announces keep the default
policy this wave. The wire-level refusal claim (a TLS-1.1-only server is
refused at `tls12`) is documented-not-executed: the sandbox has no TLS
test server.

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

### macOS SwiftUI frontend (S12-mac2, preview)

`macos-swiftui/` is a standalone SwiftPM package (zero third-party
dependencies, deployment floor macOS 12) implementing the SwiftUI
variant of the macOS frontend: a remote client of the same loopback
daemon, Material You tokens via the shared engine's Swift port, and the
gate-0010 SSE consumer contract. It does NOT replace the AppKit app
(`macos-app/`, floor macOS 10.15) — Monterey users keep the AppKit app;
macOS 12 users of the SwiftUI variant get compile-time availability
checking only (runtime verification rides the owner's promotion-gate
moment).

```sh
cd macos-swiftui
swift build          # the app (VelocityDMSwiftUIApp)
swift test           # golden-literal + frame-split + parse + sanitize suites
swift run VelocityDMSwiftUIApp   # after starting the daemon:
#   velocitydm daemon --addr 127.0.0.1:36990
```

`swift build` alone does not compile test targets — run `swift test` or
the golden tests are dead code. CI witnesses both steps in the
non-required `swiftui-frontend` job; a red `swiftui-frontend` does NOT
block tag cuts (it ships nothing). The tree README documents the
canonical-copy rule (the AppKit `Md3.swift` math is canonical for Swift
ports) and the duplication-drift risk of the three copied files.

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

See `android/README.md`. No Gradle wrapper is committed: install
Gradle 8.9 + JDK 17 + the Android SDK (`platforms;android-34`,
`build-tools;34.0.0`) and run `gradle -p android :shared:jvmTest
:app:assembleDebug` (or `assembleRelease` for the minified build).
One universal APK per release — no per-ABI splits. CI runs the same
commands on the non-required `android-frontend` job.

## GTK3 frontend (preview, S12-lin2)

The `gtk3/` tree is a standalone workspace (own `Cargo.lock`; NOT a root
workspace member — the win32/ precedent). Requires GTK3 dev libraries:

```sh
sudo apt-get install -y libgtk-3-dev pkg-config
cargo build --manifest-path gtk3/Cargo.toml --locked
cargo test  --manifest-path gtk3/Cargo.toml --locked   # headless-safe (no gtk::init)
```

CI is the canonical compile witness (non-required `gtk3-frontend` job,
ubuntu-latest + `libgtk-3-dev`): the dev sandbox has no GTK3 and no
sudo, so locally only `cargo generate-lockfile` (inside `gtk3/`) and
`cargo fmt --manifest-path gtk3/Cargo.toml --all -- --check` are
possible. The binary ships in no release asset — the egui GUI stays the
shipped Linux surface until the gtk3 promotion gate.
