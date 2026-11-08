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

## License

GPL-3.0-or-later for all first-party code in this repository.
