# VelocityDM

Fast, clean-room download manager: HTTP/HTTPS segmentation, HLS (AES-128),
BitTorrent + magnet, scheduler, browser integration, one daemon, tiny
footprint (~6 MB RAM idle, ~15 MB GUI).

```
VelocityDM 1.0.0
  engine vdm-core 1.0.0    features: http, hls+aes128, bittorrent, magnet,
                           batch, scheduler, proxy, refresh-link
```

## Highlights

| Area | What you get |
| --- | --- |
| HTTP/HTTPS | segmented transfers (guided work-stealing leases), **persistent connections across segments** (70% fewer handshakes, up to 1.7x faster on WAN RTTs), mirrors with **throughput-weighted selection** and automatic failover, retry with backoff, ETag/Last-Modified resume, checksum verification (MD5/SHA-1/SHA-256) |
| Video | HLS `.m3u8` (master playlists, variant selection, AES-128-CBC), DASH detection, video sniffing via browser extension |
| BitTorrent | `.torrent` + `magnet:` (BEP-3/9/10), HTTP trackers, rarest-first piece selection, per-file selection, seeding + built-in tracker (`velocitydm seed`) |
| Queue | priorities, max-simultaneous, daily stop-window, start-at scheduling, shutdown/command when the queue drains |
| Batch | URL patterns like `https://site/pic[001:120].jpg`, page link grabber |
| Networking | HTTP CONNECT + SOCKS5 proxies (with auth), global + per-download speed limits |
| Recover | crash-safe `.part`/`.vdmctl` state, **Refresh link** revives expired URLs |
| UI | egui GUI (Material You: dynamic color seeds, light + dark), CLI, JSON-RPC 2.0 + SSE on 127.0.0.1:36990 |
| Browsers | Chrome/Edge/Opera/Firefox extensions (video sniffing, download takeover, magnet), Safari conversion guide |

The scheduling choices follow the parallel-download research literature
(Rodriguez; Koo; guided self-scheduling; Padhye's TCP model) — see
[docs/ALGORITHMS.md](docs/ALGORITHMS.md) "Research notes" for the papers,
the mapping, and A/B measurements.

Improvements adopted from XDM, Motrix and Gopeed are itemized in
[docs/IMPROVEMENTS.md](docs/IMPROVEMENTS.md).

## Quick start

```sh
# daemon + CLI
velocitydm daemon                          # engine + JSON-RPC on 127.0.0.1:36990
velocitydm add https://example.com/big.iso --conn 8
velocitydm add https://cdn/file.m3u8 --quality best
velocitydm batch "https://site/pic[001:120].jpg"
velocitydm torrent file.torrent --files 0,2
velocitydm magnet "magnet:?xt=urn:btih:...&tr=http://tracker/announce"
velocitydm list
velocitydm refresh 3 --url https://new-link.example/file.iso

# share a file (tracker + seeder, prints the magnet link)
velocitydm torrent-create ./movie.mp4 --announce http://my.host:6969/announce
velocitydm seed ./movie.mp4

# limits & scheduler
velocitydm limits --global 5MB
velocitydm schedule --stop-start 22:00 --stop-end 06:00 --max-active 3
velocitydm schedule --shutdown-when-done

# GUI
velocitydm gui
```

The CLI auto-starts the daemon when needed.

## Layout

```
crates/
  vdm-core/         engine: http, hls, bt, scheduler, queue, limits, proxies
  vdm-rpc/          JSON-RPC 2.0 + SSE transport
  velocitydm/       daemon + CLI + egui GUI
  velocitydm-nmh/   browser native messaging host
extensions/         Chrome/Edge/Opera/Firefox (+ Safari guide)
android/            Kotlin/Compose app (segmented engine, sniffer browser)
packaging/          Inno Setup, Linux install, Flatpak, macOS bundle, portable
docs/               architecture, algorithms, clean-room notes, coverage
scripts/            e2e.sh (integration smoke), testserver.py, builders
```

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — components and data flow
- [docs/ALGORITHMS.md](docs/ALGORITHMS.md) — range pool, AIMD, token bucket, piece selection
- [docs/IMPROVEMENTS.md](docs/IMPROVEMENTS.md) — what was adopted from XDM / Motrix / Gopeed
- [docs/REQUIREMENTS-COVERAGE.md](docs/REQUIREMENTS-COVERAGE.md) — feature coverage checklist
- [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md) — clean-room implementation notes
- [docs/BUILDING.md](docs/BUILDING.md) — building, testing, cross-compiling, packaging

## License

GPL-3.0-or-later. No DRM circumvention code; media downloads use the
credentials the browser itself supplies.
