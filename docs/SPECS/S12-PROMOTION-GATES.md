# S12 promotion gates — preparation (parity matrices, packaging plan, owner-verification protocol)

**Status: PREPARED 2026-11-23 (session 13), per owner decision D3:D** (the
promotion-gate materials are prepared FIRST in the S12 queue, ahead of the
SSE wiring and the S12-mac2 SwiftUI / S12-lin2 gtk3-rs / S12-lin3 Qt
sub-stages). This document PREPARES the gates; it does not open them. Each
actual promotion remains a five-role adversarial gate (`PROTOCOL.md`) plus
the owner's runtime verification (D5:A — which happens after wave-2 close +
SSE wiring, verifying a full surface), plus the release-packaging change in
the same stage. Promotion criteria baseline: `docs/DESIGN-MATERIAL-YOU.md`
§"Promotion criteria" (five points); gate records: 0004/0005; tracker:
issue #16.

## 1. Parity matrices (grounded in the current sources, 2026-11-23; GTK3
column added at the S12-lin2 close, 2026-11-24, grounded in `gtk3/src/`)

Legend: ✓ = present in code today; ✗ = absent; ~ = partial/recorded gap.
Reference surface = the shipped egui GUI (`crates/velocitydm/src/gui.rs`).

### 1.1 Add path

| Capability | egui (ref) | Win32/D2D | Slint | Swift/AppKit | GTK3 | Android Views |
| --- | --- | --- | --- | --- | --- | --- |
| URL entry | ✓ | ✓ (EDIT) | ✓ (LineEdit) | ✓ (NSTextField) | ✓ (Entry, Enter-to-add) | ✓ (AddDialog) |
| Filename override | ✓ `add_filename` (dead field — no TextEdit binds it) | ✗ (gap) | ✗ (gap) | ✗ (gap) | ✓ (real Entry → `HttpOptions.filename`; closes the gap) | ✓ (name field) |
| Quality preset | ✓ ("best"…) | ✓ | ✓ | ✓ | ✓ (Entry, default "best") | ✗ (not applicable — Kotlin-port engine has no quality presets) |
| Speed limit | ✓ | ✓ | ✓ | ✓ | ✓ (Entry, `parse_bytes`) | ✓ (limitBps) |
| magnet: URL | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ (http/https only by design; S8 owns engine unification) |
| Local .torrent path | ✓ | ✓ | ✓ | ✗ (gap: magnet works, local paths do not) | ✓ (read → `TorrentBytes`) | ✗ (by design, as above) |
| VIEW intent auto-add | n/a | n/a | n/a | n/a | n/a | ✓ |

### 1.2 Task list + actions

| Capability | egui | Win32 | Slint | Swift | GTK3 | Android |
| --- | --- | --- | --- | --- | --- | --- |
| Pause / resume / remove | ✓ | ✓ (hit-tested drawn buttons) | ✓ | ✓ | ✓ (per-row buttons) | ✓ (row buttons) |
| Progress + speed + ETA | ✓ | ✓ | ✓ | ✓ | ✓ (ProgressBar + stats line; header total-speed banner) | ✓ (LinearProgressIndicator + stats line) |
| Per-task error display | ✓ | ✓ | ✓ | ✓ | ✓ (error AND notice lines, visibility-toggled) | ✓ (per-task error text) |
| BT peers/seeds line | ✓ | ✓ | ✓ | ✓ | ✓ (`{}p/{}s` when peers > 0) | n/a (no BT engine yet) |
| Stale rows on daemon death | handles engine stop | in-process (shares core) | in-process | ~ (recorded gap) | in-process (shares core; engine-removed tasks drop their rows in-place) | in-process (shares engine) |

### 1.3 Settings

| Setting | egui | Win32 | Slint | Swift | GTK3 | Android |
| --- | --- | --- | --- | --- | --- | --- |
| Download dir | ✓ | ✓ | ✓ | ✓ | ✓ (Entry) | ✓ (system picker via dialog) |
| Global speed limit | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ |
| Proxy (parse + display) | ✓ | ✓ | ✓ | ✓ (structured round-trip, gate 0004 fix) | ✓ (Entry, `proxy_display` push-back) | ✗ |
| Scheduler stop window | ✓ | ✓ | ✓ | ✓ | ✓ (start/end Entries) | ✗ |
| Shutdown-when-done | ✓ | ✓ | ✓ | ✓ | ✓ (CheckButton) | ✗ |
| Max active / connections | ✓ | ✓ | ✓ | ✓ | ✓ (SpinButtons 1..32 / 1..16, clamped on Apply) | ~ (connections fixed 1..8 in engine) |

Android settings gaps are NOT promotion blockers for the Android gate:
the shipped surface contract for Android v1.0.0 (Compose) never had them;
parity for the Android promotion is measured against the Compose app it
replaces (gate 0005 wording), not against the desktop settings surface.

### 1.4 Material You

| Capability | egui | Win32 | Slint | Swift | GTK3 | Android |
| --- | --- | --- | --- | --- | --- | --- |
| Token source | vdm-md3 in-process | vdm-md3 in-process | vdm-md3 in-process | Md3.swift port (constants-identical) | vdm-md3 in-process → static hex-only `GtkCssProvider` (19/27 role subset, same as Slint; zero `String` interpolation, token-only test pinned) | OS dynamic color 12+; Kotlin Md3 port below (golden-drift tripwire vs Rust baseline) |
| Seed picker | ✓ (SEEDS list) | ✓ | ✓ | ✓ | ✓ (SEEDS cycle button + name label) | ✓ (12+ wallpaper; pre-12 seed list) |
| Light/dark toggle | ✓ | ✓ | ✓ | ✓ | ✓ (provider reload in place — registered once, no stacking) | OS-follows (12+) / toggle pre-12 |

### 1.5 Engine integration

| Aspect | egui | Win32 | Slint | Swift | GTK3 | Android |
| --- | --- | --- | --- | --- | --- | --- |
| Core binding | in-process | in-process | in-process | remote JSON-RPC (loopback, trust-boundary compliant) | in-process | in-process Kotlin-port engine (S8 owns unification) |
| Progress refresh | repaint loop | `SetTimer` 200 ms | Timer | SSE stream (gate 0010) + stream-state recovery | glib timeout 200 ms poll (in-process; SSE change is Swift-only) | ViewModel flow |

## 2. Packaging plan (what changes, per surface, at promotion)

Current release asset composition (release.yml, verified live
2026-11-23): windows zips = egui `velocitydm.exe` + `velocitydm-nmh.exe`;
macos zips = egui `velocitydm` + `velocitydm-nmh` (osxcross-built, x64 +
aarch64); linux tars = egui `velocitydm` + `velocitydm-nmh` (gnu + musl);
-android = APK; the win7 payload cross-compiles headless bins; the
extensions ship their four zips. None of the preview frontends is in any
asset today.

| Surface | Ships as | Mechanism | Constraints |
| --- | --- | --- | --- |
| Win32/D2D | `velocitydm-win32.exe` joins the two windows zips | release.yml windows job gains a `frontend/win32` build step (standalone workspace, MSVC) | frozen v1.0.0-family releases untouched — promotion targets the NEXT v-tag; `v*` tag ruleset (creation+update restricted) stays active; release.yml `workflow_dispatch` remains forbidden until the gate passes |
| Slint | `velocitydm-slint` joins both linux tars | it is already a root-workspace member: add the bin to the two `cp` staging lines (gnu + musl) | same ruleset; the musl build must keep the slint software-renderer path working |
| Swift/AppKit | `VelocityDM.app` bundle joins the two macos zips | `packaging/macos/bundle-macos.sh` / `make-app.sh` on a macos-latest runner (SwiftPM; the osxcross job cannot build Swift) | a NEW macos build job in release.yml at promotion; bundle icon/Info.plist/minimum macOS 10.15 from `macos-app/` |
| Android Views | the Views APK replaces the Compose APK in the `-android` release | the existing android release job already builds `assembleRelease` from the tree — the change is the versionCode/versionName bump + the promotion record | versionCode 1 / versionName 1.0.0 frozen until the gate (gate 0005 rule); record the release-mode size delta (debug APK 6.6 MB vs 10.5 MB Compose at gate 0005; R8 release already exercised in CI) |

General rules (all surfaces): the six v1.0.0-family releases stay frozen
(assets already attached are never clobbered); every packaging change
lands in the SAME stage-commit as its promotion gate record; the
`v*` ruleset (27298722) is the procedural backstop until the moment of
promotion; no promotion ships without the owner's runtime-verification
result recorded below.

## 3. Owner-verification protocol (D5:A — the runtime half of each gate)

**Timing (D5:A):** after wave-2 close + SSE wiring — i.e. after the Swift
client consumes SSE, verify a FULL surface in one pass. Wave 2 closed
2026-11-23; SSE wiring is the next gated sub-stage after this
preparation. The moment is the owner's; this protocol is what runs then.

**What the owner needs:** real hardware per OS (the sandbox has no
displays — CI link success is not run evidence), one working daemon or
bundled frontend build per surface, and any local file host for a test
download.

**Per-surface checklist** (each row is pass/fail + a one-line note; any
fail blocks that surface's promotion and files an issue with repro
steps; partial passes promote nothing — the gate re-runs after a fix):

1. **Windows 10/11 (Win32/D2D)**: launch; window renders with MD3 chrome;
   DPI: drag between monitors with different scaling — no 1×1 collapse,
   chrome scales with content; add a URL (with limit) → progress, pause,
   resume, complete, file verifiable; magnet: URL resolves and downloads;
   seed button cycles palettes; dark toggle; settings panel: proxy text
   round-trips, stop window persists after restart; quit → process exits
   (no orphan tray/pid).
2. **Linux (Slint)**: launch; same add/pause/resume/complete flow; local
   .torrent path; theme + settings as above; musl build launches on a
   musl host if available (else note honestly).
3. **macOS (Swift/AppKit)**: `swift build` product or the bundled .app
   against the running daemon; add URL/magnet; rows update at a
   human-smooth cadence; kill the daemon → UI degrades gracefully (no
   stale ghost rows — the recorded gap must be FIXED before the gate,
   not after); proxy structured round-trip; seed/dark; standard mac
   conventions (menu bar, cmd-Q).
4. **Android (Views)**: install APK on a real device (minSdk floor per
   manifest); VIEW-intent auto-add from the browser; add dialog (URL,
   filename, limit); pause/resume/remove; OS dynamic color on 12+,
   seeded scheme below 12; app survives screen rotation + process death
   (service restarts); R8 release build (CI already builds it — the
   owner verifies RUNTIME).
5. **Extensions (already shipped, re-check at the same moment):** one
   Chromium-family takeover end-to-end including the B6 confirmation
   page + decline → browser download continues; Firefox NM host
   reconnect after a core restart (B4).

**Evidence recording:** one comment per surface on issue #16 with the
checklist, pass/fail, environment (OS version, device/arch), and any
screenshots; the harness treats those comments as the authoritative
runtime evidence the promotion gate cites.

**Escalation:** a failed item → issue on the `v1.1.0 - roadmap` milestone
with repro → the surface's promotion gate is blocked until the fix lands
and the checklist row passes. The agent cannot substitute for any of
these steps (honest boundary: no displays, no devices here).

## 4. Gap register (promotion-blocking, from gates 0004/0005, verified current 2026-11-23)

| Gap | Surface | Status at preparation | Owning step |
| --- | --- | --- | --- |
| Filename override in add dialog | Win32, Slint, Swift | **CLOSED by GTK3** 2026-11-24 (real Entry → `HttpOptions.filename`, `gtk3/src/main.rs`); Win32/Slint/Swift still absent (verified: no filename edit/property/field) — and the egui reference's ✓ is a dead field (no TextEdit binds `add_filename`) | fix BEFORE each remaining surface's gate (parity criterion 3) |
| Local .torrent path add | Swift | still absent (magnet works) | fix before the Swift gate |
| Enter-to-add + TAB traversal | Win32 | still absent | fix before the Win32 gate |
| Per-paint brush/text-format churn | Win32 | still present | cache on retheme; fix before the Win32 gate |
| Stale rows on daemon death | Swift | fixed in source 2026-11-24 (banner "daemon unreachable — rows stale" + frozen rows + 2 s recovery probe; compile witness CI swift-frontend) | runtime rows at the D5:A verification moment |
| SSE wiring (replace 200 ms poll) | Swift | DONE 2026-11-24 (gate 0010: spec `S12-SSE-WIRING.md` + implementation a0f801f, CI 11/11 green incl. swift-frontend) | runtime verification rides the D5:A moment; precedes the per-surface promotions |
| Release-mode APK size delta | Android | record at gate (debug was 6.6 MB vs 10.5 MB Compose) | measurement at the Android gate |

## 5. Gate order (the queue this preparation leaves behind)

1. **SSE wiring (Swift)** — its own five-role gate; the
   owner-verification moment follows it per D5:A.
2. **Owner runtime verification of the full surface** (§3 above).
3. Per-surface promotions, each a five-role gate + §2 packaging change in
   the same stage: Win32 → Slint → Swift → Android (any order the owner
   prefers once the checklists pass; each cites its §3 evidence).
4. Remaining frontends, each with its own five-role gate, per D3:D's
   queue order: ~~S12-lin2 gtk3-rs~~ **DONE (preview) 2026-11-24 under
   gate 0012** (`ASSESSMENTS/0012-s12-lin2-gtk3.md`, CI 12/12 green at
   6c16845 — 8 required + 4 preview incl. `Linux frontend (GTK3)`;
   promotion still owes its own gate + the §3 checklist); next:
   S12-mac2 SwiftUI, then S12-lin3 Qt.

Prepared by session 13 (2026-11-23) as the D3:D preparation deliverable;
committed before any of the steps above (no implementation rides along).
