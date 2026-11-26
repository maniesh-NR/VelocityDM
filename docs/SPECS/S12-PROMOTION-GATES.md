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
column added at the S12-lin2 close, 2026-11-24, grounded in `gtk3/src/`;
SwiftUI column added at the S12-mac2 close, 2026-11-24, grounded in
`macos-swiftui/Sources/` — the "Swift"/"Swift/AppKit" column remains the
AppKit tree `macos-app/`)

Legend: ✓ = present in code today; ✗ = absent; ~ = partial/recorded gap.
Reference surface = the shipped egui GUI (`crates/velocitydm/src/gui.rs`).

### 1.1 Add path

| Capability | egui (ref) | Win32/D2D | Slint | Swift/AppKit | GTK3 | SwiftUI | Qt Quick | Android Views |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| URL entry | ✓ | ✓ (EDIT) | ✓ (LineEdit) | ✓ (NSTextField) | ✓ (Entry, Enter-to-add) | ✓ (TextField, Enter-to-add via `.onSubmit`) | ✓ (TextInput, Enter-to-add via `onAccepted`) | ✓ (AddDialog) |
| Filename override | ✓ `add_filename` (dead field — no TextEdit binds it) | ✗ (gap) | ✗ (gap) | ✗ (gap) | ✓ (real Entry → `HttpOptions.filename`; closes the gap) | ✓ (real bound field → `options.filename`; closes the gap in this tree) | ✓ (real TextInput → `HttpOptions.filename`; closes the gap in this tree) | ✓ (name field) |
| Quality preset | ✓ ("best"…) | ✓ | ✓ | ✓ | ✓ (Entry, default "best") | ✓ (Entry, default "best") | ✓ (TextInput, default "best") | ✗ (not applicable — Kotlin-port engine has no quality presets) |
| Speed limit | ✓ | ✓ | ✓ | ✓ | ✓ (Entry, `parse_bytes`) | ✓ (Entry, `parseBytes`) | ✓ (TextInput, `parse_bytes`) | ✓ (limitBps) |
| magnet: URL | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ (→ top-level `magnet` param) | ✓ (→ `BtSpec::Magnet` in-process) | ✗ (http/https only by design; S8 owns engine unification) |
| Local .torrent path | ✓ | ✓ | ✓ | ✗ (gap: magnet works, local paths do not) | ✓ (read → `TorrentBytes`) | ✓ (typed path field → top-level `torrentPath`; closes the gap in this tree) | ✓ (path/existing-file → `TorrentBytes` in-process) | ✗ (by design, as above) |
| VIEW intent auto-add | n/a | n/a | n/a | n/a | n/a | n/a | n/a | ✓ |

### 1.2 Task list + actions

| Capability | egui | Win32 | Slint | Swift (AppKit) | GTK3 | SwiftUI | Qt Quick | Android |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Pause / resume / remove | ✓ | ✓ (hit-tested drawn buttons) | ✓ | ✓ | ✓ (per-row buttons) | ✓ (per-row buttons) | ✓ (per-row MouseArea buttons, can-pause/can-resume gated) | ✓ (row buttons) |
| Progress + speed + ETA | ✓ | ✓ | ✓ | ✓ | ✓ (ProgressBar + stats line; header total-speed banner) | ✓ (linear `ProgressView` clamped 0…1 + stats line; header total-speed banner) | ✓ (drawn linear bar (permille) + stats line; header total-speed banner) | ✓ (LinearProgressIndicator + stats line) |
| Per-task error display | ✓ | ✓ | ✓ | ✓ | ✓ (error AND notice lines, visibility-toggled) | ✓ (error AND notice lines, visibility-toggled) | ✓ (error AND notice lines, visibility-toggled) | ✓ (per-task error text) |
| BT peers/seeds line | ✓ | ✓ | ✓ | ✓ | ✓ (`{}p/{}s` when peers > 0) | ✓ (`Xp/Ys` when peers > 0) | ✓ (`{n}p/{n}s` for BT kinds) | n/a (no BT engine yet) |
| Stale rows on daemon death | handles engine stop | in-process (shares core) | in-process | ✓ fixed in source 2026-11-24 (stale banner + frozen rows + 2 s probe; compile witness `swift-frontend`) | in-process (shares core; engine-removed tasks drop their rows in-place) | ✓ (stale banner + frozen rows + 2 s probe; the gate-0010 consumer contract restated — SSE consumer) | in-process (shares core; membership resets drop removed rows) | in-process (shares engine) |

### 1.3 Settings

| Setting | egui | Win32 | Slint | Swift (AppKit) | GTK3 | SwiftUI | Qt Quick | Android |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Download dir | ✓ | ✓ | ✓ | ✓ | ✓ (Entry) | ✓ (Entry) | ✓ (TextInput) | ✓ (system picker via dialog) |
| Global speed limit | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ (Entry, `parseBytes`) | ✓ (TextInput, `parse_bytes`) | ✗ |
| Proxy (parse + display) | ✓ | ✓ | ✓ | ✓ (structured round-trip, gate 0004 fix) | ✓ (Entry, `proxy_display` push-back) | ✓ (structured round-trip, `RpcProxy.parse`; "none" clears) | ✓ (TextInput, `proxy_display` push-back; "none" clears) | ✗ |
| Scheduler stop window | ✓ | ✓ | ✓ | ✓ | ✓ (start/end Entries) | ✓ (start/end Entries, `isHHMM`-validated) | ✓ (from/to TextInputs, `is_hhmm` both-ends rule) | ✗ |
| Shutdown-when-done | ✓ | ✓ | ✓ | ✓ | ✓ (CheckButton) | ✓ (Toggle) | ✓ (toggle button) | ✗ |
| Max active / connections | ✓ | ✓ | ✓ | ✓ | ✓ (SpinButtons 1..32 / 1..16, clamped on Apply) | ✓ (1…16 / 1…32, clamped on Apply — the clamps the AppKit path lacks are implemented here) | ✓ (TextInputs 1…32 / 1…16, clamped on Apply) | ~ (connections fixed 1..8 in engine) |

Android settings gaps are NOT promotion blockers for the Android gate:
the shipped surface contract for Android v1.0.0 (Compose) never had them;
parity for the Android promotion is measured against the Compose app it
replaces (gate 0005 wording), not against the desktop settings surface.

### 1.4 Material You

| Capability | egui | Win32 | Slint | Swift (AppKit) | GTK3 | SwiftUI | Qt Quick | Android |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Token source | vdm-md3 in-process | vdm-md3 in-process | vdm-md3 in-process | Md3.swift port (constants-identical) | vdm-md3 in-process → static hex-only `GtkCssProvider` (19/27 role subset, same as Slint; zero `String` interpolation, token-only test pinned) | Md3.swift copy (math verbatim; SwiftUI `Color` bridge; honest 18/27 subset; 288-value golden tripwire vs the Rust baseline) | vdm-md3 in-process → hex-string qproperties (typed `format!("#{:06x}")` only; honest 16/27 role subset with the `ROLE_SUBSET` tripwire) | OS dynamic color 12+; Kotlin Md3 port below (golden-drift tripwire vs Rust baseline) |
| Seed picker | ✓ (SEEDS list) | ✓ | ✓ | ✓ | ✓ (SEEDS cycle button + name label) | ✓ (8-seed cycle button + name label; `vdm.seed`) | ✓ (SEEDS cycle button + name label; `ui.json`) | ✓ (12+ wallpaper; pre-12 seed list) |
| Light/dark toggle | ✓ | ✓ | ✓ | ✓ | ✓ (provider reload in place — registered once, no stacking) | ✓ (toggle; `vdm.dark`) | ✓ (toggle; qproperty re-apply + `ui.json`) | OS-follows (12+) / toggle pre-12 |

### 1.5 Engine integration

| Aspect | egui | Win32 | Slint | Swift (AppKit) | GTK3 | SwiftUI | Qt Quick | Android |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Core binding | in-process | in-process | in-process | remote JSON-RPC (loopback, trust-boundary compliant) | in-process | remote JSON-RPC (loopback, trust-boundary compliant; cookie-less sessions + redirect refusal) | in-process (fixed-port loopback RPC server for external clients; cxx-qt bridge) | in-process Kotlin-port engine locally + OPTIONAL remote control of a desktop daemon (S8-W1: authenticated TLS + bearer token, TOFU-pinned, SSE consumer per gate-0010; unification onto the Rust core remains S8-W2) |
| Progress refresh | repaint loop | `SetTimer` 200 ms | Timer | SSE stream (gate 0010) + stream-state recovery | glib timeout 200 ms poll (in-process; SSE change is Swift-only) | SSE stream (the gate-0010 consumer contract restated; stream-state recovery incl. launch-with-dead-daemon idempotency) | QML `Timer` 200 ms poll (SSE deferred — recorded in RUNBOOK/README) | ViewModel flow |

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
| Filename override in add dialog | Win32, Slint, Swift | **CLOSED by GTK3** 2026-11-24 (real Entry → `HttpOptions.filename`, `gtk3/src/main.rs`); **closed in the S12-mac2 SwiftUI tree** 2026-11-24 (real bound field → `options.filename`, `macos-swiftui/Sources/VelocityDMSwiftUIApp/RpcClient.swift`); **closed in the S12-lin3 Qt tree** 2026-11-25 (real TextInput → `HttpOptions.filename`, `qt/src/app_model.rs`); Win32/Slint still absent, and the AppKit tree still carries the gap (its promotion owns it) — the egui reference's ✓ is a dead field (no TextEdit binds `add_filename`) | fix BEFORE each remaining surface's gate (parity criterion 3) |
| Local .torrent path add | Swift | **closed in the S12-mac2 SwiftUI tree** 2026-11-24 (typed path field → top-level `torrentPath`, `macos-swiftui/Sources/VelocityDMSwiftUIApp/RpcClient.swift`); macos-app still carries both gaps (its promotion owns them — magnet works there, local paths do not) | fix before the AppKit gate |
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
   promotion still owes its own gate + the §3 checklist);
   ~~S12-mac2 SwiftUI~~ **DONE (preview) 2026-11-24 under gate 0013**
   (`ASSESSMENTS/0013-s12-mac2-swiftui.md`, spec
   `docs/SPECS/S12-MAC2-SWIFTUI.md` + standalone `macos-swiftui/` tree,
   CI 13/13 green at d11dd1a5 — 8 required + 5 preview incl. `macOS
   frontend (SwiftUI)`; closes the filename + torrentPath gaps in that
   tree; promotion still owes its own gate + the §3 checklist);
   ~~S12-lin3 Qt~~ **DONE (preview) 2026-11-25 under gate 0017**
   (`harness/ASSESSMENTS/0017-s12-lin3-qt.md`, spec
   `docs/SPECS/S12-LIN3-QT.md` + standalone `qt/` tree — cxx-qt 0.10.0,
   QAbstractListModel via the upstream `#[base]`+`#[cxx_override]`+
   inherited-`dataChanged` pattern; CI green incl. the new `Linux
   frontend (Qt)` preview job after the namespace evidence-then-fix
   cycle; closes the filename-override gap in that tree; promotion
   still owes its own gate + the §3 checklist + a §2 packaging row);
   the S12 sub-stage queue is now EMPTY — the owner's D5:A runtime
   verification moment (§3) precedes the per-surface promotions, and
   the remaining roadmap stages (S8 LAN daemon + Android unification,
   S9 Safari app extension, S10 Flathub) own their gates.

Prepared by session 13 (2026-11-23) as the D3:D preparation deliverable;
committed before any of the steps above (no implementation rides along).
