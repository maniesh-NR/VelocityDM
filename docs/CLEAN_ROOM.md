# Clean-room implementation notes

VelocityDM is a clean-room implementation: none of its code derives from
Internet Download Manager (IDM), XDM, Motrix, Gopeed, aria2 or any other
download manager. The project was specified from publicly documented
protocol standards and public feature descriptions:

| Area | Specification source |
| --- | --- |
| HTTP/1.1 semantics, Range, chunked, keep-alive | RFC 9110 / RFC 9112 (and the older RFC 7231/7233 behavior they codify) |
| TLS | rustls (MIT/Apache-2.0 library) with the webpki root store |
| SOCKS5 | RFC 1928; HTTP proxy CONNECT per RFC 9110 §9.3.6 |
| HLS, AES-128 media encryption | Apple HTTP Live Streaming spec (2nd edition, draft-pantos-hls-rfc8216) |
| BitTorrent bencode / peer wire | BEP-3 |
| Extension handshake, ut_metadata | BEP-10, BEP-9 |
| Magnet URIs | the `magnet:` scheme (MAGNET-URI, with base32 per RFC 4648) |
| Material Design 3 | public Material 3 color/type guidance (dark baseline palette) |
| IDM behavioral observation (S11) | `RESEARCH-IDM-STUDY.md` — functional descriptions only, never code (see the S11 section below) |

Product ideas taken from public feature pages of XDM, Motrix and Gopeed
(segmentation behavior, refresh-link, scheduler, batch patterns, per-file
torrent selection, seeding, etc.) are *ideas*, re-implemented from scratch;
see IMPROVEMENTS.md for the mapping and citations.

Third-party code used as **libraries** (not derived-from): rustls, serde,
serde_json, sha1, sha2, md-5, aes, hex, clap, eframe/egui, tempfile
(dev-only); OkHttp on Android. All are permissive (MIT/Apache-2.0/ISC) and
GPL-compatible.

## No DRM circumvention

The engine downloads only what a URL plus user-supplied headers (cookies,
referer, user-agent — captured by the browser extension from the user's own
session) legitimately return. There is no DRM code, no key extraction, no
protocol circumvention. Encrypted HLS segments are decrypted with the key
the server itself hands out in the playlist.

## S11 — reference-product behavioral observation (T2, 2026-11-21)

Per the S11 brief's clean-room hygiene rules, a static (T2) study of the
reference product's official installer (IDM 6.43 build 18, fingerprinted
and re-verified before work began) was performed on 2026-11-21, owner-
authorized in-chat. Rules honored, restated:

- All reverse-engineering artifacts (the installer binary, extracted
  payloads, disassembly/decompiler output) lived only in a throwaway
  directory outside this repository and were wiped after distillation.
  None of them was ever committed, pushed, or backed up.
- Findings entered the repository exclusively as functional behavior
  descriptions in our own words (`RESEARCH-IDM-STUDY.md`): no code, no
  data structures chosen for implementation reasons, no resources, no
  string tables, no UI were copied.
- Any technique adopted from that document still requires: the behavior
  spec committed BEFORE the implementing commit (spec-first barrier),
  a five-role incorporation gate, and a measured A/B win via
  `scripts/bench-perf.sh` with no regressions in the existing batteries.
- Adoption backlog items derived from T2 observation are flagged for
  stricter spec review at those gates.

**S11-i adoption record (wave 1, 2026-11-21):** the first three backlog
items (B1 cross-connection content audit with single-connection
recovery, B2 optional connections, B9 file-lock classification/retry)
were adopted under gate 0006
(`harness/ASSESSMENTS/0006-s11i-adoption-wave1.md`, zero BLOCK, all
conditions folded). The barrier was honored in practice: behavior specs
in our own words were committed in `docs/SPECS/S11-ADOPTIONS.md` BEFORE
the implementing commit; the implementation was written from that spec
(constants, thresholds and mechanisms chosen independently — 256 KiB
audit blocks, 8 MiB stride, spawn/retire headroom factors and
persistence counts, per-OS lock-code tables — none of them derived from
the reference product's internals); the T2 provenance flag rides the
spec header and each item. Future waves follow the same barrier.

## License

GPL-3.0-or-later for all first-party code in this repository.
