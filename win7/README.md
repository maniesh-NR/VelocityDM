# Windows 7 payload

Windows 7 ships with a **headless** VelocityDM payload: the daemon, the CLI,
and the native messaging host. The browser extension popup is the UI; the
egui desktop GUI requires rustc 1.76+ and therefore cannot target Windows 7.

## Toolchain

- `rustup toolchain install 1.75.0` (the last toolchain supporting Windows 7
  as a tier-2 target environment).
- Targets: `x86_64-pc-windows-gnu` and/or `i686-pc-windows-gnu`.
- `cargo-zigbuild` + Zig 0.13 provide the mingw linker without a separate
  MSYS2 install: `cargo install cargo-zigbuild` (Zig installed outside this
  repository).

## Build

From the repository root (NOT the win7/ directory; the parent workspace's
lockfile is authoritative and every pinned dependency has MSRV <= 1.75):

```sh
rustup target add x86_64-pc-windows-gnu i686-pc-windows-gnu
cargo +1.75.0 zigbuild --release --workspace \
    --target x86_64-pc-windows-gnu
cargo +1.75.0 zigbuild --release --workspace \
    --target i686-pc-windows-gnu
```

Artifacts land in
`target/<target>/release/{velocitydm.exe, velocitydm-nmh.exe}`.

## Packaging

Use `packaging/windows/velocitydm.iss` (Inno Setup) with the Win7-built
binaries, or ship the portable layout (`packaging/portable/run-portable.bat`
next to the exe files). Register the NMH either through the Inno Setup
registry keys or manually per docs/EXTENSIONS.md.

## Notes

- The `hls-aes` and `tls` features build on 1.75 (RustCrypto AES, rustls 0.21
  with ring); no feature is disabled for Windows 7.
- If cargo 1.75 rejects a transitive dependency's edition, regenerate the
  lockfile with `cargo +1.75.0 update` so resolution happens under 1.75
  constraints; the workspace pins make this the expected path.
