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

Build the **win7 workspace** (authoritative for the Windows 7 payload: its
`Cargo.toml` pins every dependency to an MSRV <= 1.75 version and
`win7/Cargo.lock` is resolved under cargo 1.75 — the root workspace lock may
resolve newer transitive crates that require edition2024, which cargo 1.75
cannot read). The `win7/crates/*` sources are identical to the root crates.

```sh
rustup target add x86_64-pc-windows-gnu i686-pc-windows-gnu --toolchain 1.75.0
cargo +1.75.0 zigbuild --manifest-path win7/Cargo.toml --locked --release \
    -p velocitydm --no-default-features --features tls -p velocitydm-nmh \
    --target x86_64-pc-windows-gnu
cargo +1.75.0 zigbuild --manifest-path win7/Cargo.toml --locked --release \
    -p velocitydm --no-default-features --features tls -p velocitydm-nmh \
    --target i686-pc-windows-gnu
```

Artifacts land in
`win7/target/<target>/release/{velocitydm.exe, velocitydm-nmh.exe}`
(headless daemon + CLI + NMH; build the GUI from the root workspace with a
current toolchain instead).

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
