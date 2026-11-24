# S12-mac2 — SwiftUI frontend for newer macOS (behavior spec)

**Stage:** S12-mac2 (gate 0013, `harness/ASSESSMENTS/0013-s12-mac2-swiftui.md`,
5× PROCEED-WITH-CONDITIONS, zero BLOCK). **Queue position:** next in the
S12 sub-stage order per owner decision D3:D, after S12-lin2 GTK3
(gate 0012). **No S11/T2 linkage** — zero engine code; no CLEAN_ROOM
row owed; bench A/B not owed. **Preview surface:** ships in NO release
asset until its own promotion gate (owner runtime verification per
D5:A + packaging decision). This spec is committed BEFORE any
implementing commit.

## 1. Scope

### 1.1 In scope
- New standalone SwiftPM package `macos-swiftui/` (sibling of
  `macos-app/`), one executable target `VelocityDMSwiftUIApp`,
  `swift-tools-version:5.9`, `platforms: [.macOS(.v12)]`, **zero
  third-party dependencies** (no `Package.resolved` — SwiftPM writes
  none for a dependency-free package; nothing to commit).
- SwiftUI `@main App` lifecycle. All UI in SwiftUI; AppKit appears
  ONLY via `@NSApplicationDelegateAdaptor` for the termination hook
  and standard conveniences SwiftUI does not cover.
- Material You chrome from a **copy** of `Md3.swift` (the Swift port
  of the shared `vdm-md3` token engine) — math core copied verbatim;
  the NSColor/CGColor bridge extensions are REPLACED by a SwiftUI
  `Color` bridge; the header claim corrected to the honest subset
  (18 of 27 roles — see rule 12).
- Remote client of the shared Rust core over the documented loopback
  JSON-RPC protocol (`docs/RPC.md`, 127.0.0.1:36990) + SSE event
  stream per rule 8. The daemon's trust boundary (Host/Origin/
  Content-Type three-rule check) is the real boundary and is NOT
  touched by this stage.
- Copies of `RpcClient.swift` and `SseClient.swift` from
  `macos-app/`, ADAPTED per rules 9-11 (never claimed as "identical
  reuse").
- Parity surface: `docs/SPECS/S12-PROMOTION-GATES.md` §1.1–§1.5 Swift
  column as the floor, PLUS the two registered Swift gaps closed as
  real UI: filename override (`downloads.add` `options.filename`) and
  local `.torrent` add (`downloads.addTorrent` `torrentPath`).
- Pure-logic test target (`macos-swiftui/Tests/`): MD3 golden
  literals (8 seeds × light/dark, hardcoded hex copied from
  `scripts/../android/shared` baseline `md3-tokens-baseline.json`),
  frame-split behavior, byte-parse, proxy parse, sanitize payloads.
- Additive CI: non-required `swiftui-frontend` job (macos-latest,
  `swift build` + `swift test`). Additive `.gitignore` lines for
  `/macos-app/.build` and `/macos-swiftui/.build`. `docs/BUILDING.md`
  gains a SwiftUI section; the tree gains a README.

### 1.2 Out of scope (explicit)
- `release.yml` — byte-untouched; `workflow_dispatch` stays forbidden
  until the promotion gate (manual dispatch would build branch HEAD
  and clobber frozen assets).
- The `v*` tag ruleset (id 27298722) — untouched.
- The 8 required CI checks — untouched; their display names are
  byte-exact protection contexts and MUST NOT be renamed.
- No `macos-app/**` change rides the S12-mac2 revert unit — the AppKit
  app is a built, gated tree; refactoring it to a library+executable
  split is OUT (its own gated sub-stage if ever wanted; recorded as
  the promotion-gate end-state for single-source-of-truth reuse).
  ONE exception, as its own separate tiny commit (NOT part of the C2
  revert unit): the two-line Md3.swift header-truth fix the FALS
  condition demands (the "Full Material Design 3 color-role set"
  claim is false advertising — 18 of 27 roles).
- Engine (`crates/**`), `win7/`, `android/`, `extensions/` — untouched.
- No e2e additions; push-main-only; no bench A/B (no engine deltas).

## 2. Rendering safety (attacker-controlled text)

Every daemon-relayed string renders through ONE helper that calls
`Text(verbatim:)` — the SwiftUI analog of the gtk3 `set_text()` rule
(gate 0012). Daemon-relayed strings are: task `name`, `url`, `dir`,
`outPath`, `error`, `notice`, the SSE `task-completed` `path`, RPC and
transport error messages, and any banner embedding them.

- BANNED: string interpolation inside `Text("…\(x)…")` (binds to the
  `LocalizedStringKey` initializer, which parses and renders Markdown —
  on macOS 12+ Markdown links render as tappable Links: a hostile task
  name `[Invoices.pdf](https://evil.example)` becomes a phishing
  element); `Text` concatenation (`+`) with daemon-derived parts;
  `AttributedString(markdown:)` on daemon data; any tappable link or
  URL-opening control built from a daemon string in v1.
- A control-character sanitize helper (the gtk3 `display_text`
  analog: literal rendering, C0/DEL controls replaced with spaces,
  first-write content never logged) wraps the helper; unit tests pin
  the payload classes (markdown link, HTML-ish span, control chars,
  padding).
- Alerts/tooltips take `Text(verbatim:)` explicitly, never `String`
  titles (which bind to `LocalizedStringKey`).

## 3. Material You tokens

- Seed → scheme via the copied `Md3.swift` math (constants-identical
  to `crates/vdm-md3`; golden-literal tests pin 8 seeds × light/dark).
- **Honest subset row: 18 of 27 Scheme roles** (same subset the
  AppKit tree carries; NOT full token parity). The copied header is
  corrected; the `macos-app` copy's identical false header is
  corrected in the separate header-truth commit (rule 1.2 exception).
- Seeds: the 8-entry `Md3Seeds` list (matches `vdm_md3::SEEDS`), cycle
  button + name label; light/dark toggle; persisted under the SAME
  UserDefaults keys as the AppKit app: `vdm.seed`, `vdm.dark`.
- Styling consumes `Md3Scheme` fields only; no daemon string ever
  reaches a color/stylesheet.

## 4. RPC client (adapted copy)

- `RpcClient.swift` copied from `macos-app/` and adapted:
  - `@MainActor`-isolated (the `nextId` counter is caller-thread
    mutated in the original; SwiftUI task contexts race it — all RPC
    calls execute on the main actor, which is also what makes
    SwiftUI model updates safe).
  - `add(url:quality:limitBps:filename:)` — `filename` inside the
    `downloads.add` `options` object (`HttpOptions`).
  - `addTorrent(magnet:)` AND `addTorrent(path:)` — params at TOP
    LEVEL exactly as the daemon parses them (`magnet` or
    `torrentPath`, plus `outDir`, `filePriorities`, `rateLimitBps`);
    the AppKit copy's `"options": [:]` nesting on addTorrent is a
    silently-ignored key and MUST NOT be copied.
  - `TaskSnapshot` gains `notice: String?` (additive field,
    `docs/RPC.md` documents it; gtk3 renders it).
- Transport hardening pins (new-tree baseline): dedicated
  `URLSessionConfiguration` with `httpShouldSetCookies = false` and
  `httpCookieAcceptPolicy = .never`; redirect refusal
  (`willPerformHTTPRedirection` → cancel — a redirect from the
  loopback daemon can only be a hostile local port-owner); base URL
  hardcoded `127.0.0.1:36990` (no user-configurable non-loopback
  address in v1); ATS posture: no exceptions now; if one ever proves
  necessary it is loopback-scoped (`NSAllowsLocalNetworking`), never a
  global disable.
- Error surfacing: every RPC/transport failure reaches the UI verbatim
  through the rule-2 helper (the AppKit `"! \(e)"` field precedent);
  no swallowing, no silent fallback, no auto-retry of add paths.

## 5. Add path (parity rows §1.1)

- URL entry (placeholder does not over-promise beyond what is wired),
  filename override (real bound field → `options.filename`), quality
  preset (default "best"), speed limit (`parseBytes`, empty = none),
  `magnet:` URL, local `.torrent` path via `torrentPath` — populated
  ONLY from an explicit user action (typed path field; no file-picker
  dialog is owed — gtk3 precedent), never parsed from daemon or
  network data. Enter-to-add via `.onSubmit`; add errors surface in a
  dedicated error line; success clears the fields.

## 6. Task list + actions (parity rows §1.2)

- In-place row updates keyed by task id: name, state chip, progress
  fraction (clamped 0...1), stats line (speed/ETA/size + `Xp/Ys` when
  peers > 0), error AND notice lines (visibility-toggled, rule-2
  helper), pause/resume/remove buttons.
- Total-speed banner; honest empty state.

## 7. Settings (parity rows §1.3)

Download dir; global speed limit; proxy structured round-trip
(`RpcProxy.parse` — http/socks5, "none" clears); scheduler stop window
(start/end); shutdown-when-done; connections (1...32) and max-active
(1...16) with clamps applied on save (honest: the AppKit settings path
has no clamps — the SwiftUI tree implements them rather than claiming
them); Apply starts from the live config so unexposed fields survive;
settings + limits + queue round-trips through the RPC client.

## 8. SSE wiring (the gate-0010 consumer contract, restated for this tree)

`docs/SPECS/S12-SSE-WIRING.md` is scoped to `macos-app/` and binds no
new consumer — this spec restates the full contract for the SwiftUI
tree:

- `SseClient.swift` copied (Foundation-only) with `stop()` wired into
  the SwiftUI termination path via `@NSApplicationDelegateAdaptor` →
  `applicationWillTerminate` (no orphaned stream task at exit).
- GET `http://127.0.0.1:36990/events`, `Accept: text/event-stream`, no
  `Origin`/`Referer` header ever; dedicated cookie-less session;
  redirect refusal; 24 h request/resource timeouts (idle streams have
  no heartbeats).
- `\n\n` frame split with straddling reads; **1 MiB buffer cap that
  DROPS the buffer on a terminator-less overflow** (never grows,
  never throws — the memory-exhaustion defense); lenient `data:`-only
  parsing; non-JSON / non-`type` frames ignored, never fatal;
  non-200 / non-`text/event-stream` ⇒ terminal.
- Event handling: `tasks` ticks merge in place keyed by id (the four
  tracked fields only) — metadata-only drift marks dirty for a
  coalesced re-list; all other events coalesce to re-lists at
  150 ms; **ticks omit removals — removal convergence rides
  `task-removed` → re-list**; unknown ids append, never reorder; full
  re-list precedes any merge after (re)connect; idle daemon ⇒ zero
  RPC.
- Terminal stream state (framework-neutral behavior, all pinned):
  `reachable(false)`; the total-speed label swaps to the stale wording
  ("daemon unreachable — rows stale"); rows FROZEN (no merges,
  no re-lists mutate the model while down); a 2 s repeating
  `downloads.list` probe whose failures keep the banner and never
  mutate the model; recovery = first probe success → stream reopen →
  head flips state → one full re-list precedes any merge; the state
  machine is idempotent both ways INCLUDING launch-with-dead-daemon
  (initial false→false "transition" still opens the fallback). The
  SSE-spec §5.3 "(reason updated)" wording is omitted — the code
  truth is that probe failures update nothing.

## 9. Deployment floor

`platforms: [.macOS(.v12)]` — pinned once, with rationale: SwiftUI
`@main App` requires macOS 11+; `.task`/`.onSubmit`/`.refreshable`
require 12+; 13-only APIs (`NavigationStack`, `Grid`,
`LabeledContent`, `.scrollContentBackground`) are FORBIDDEN unguarded
(the CI witness fails the build on a 12 target if one is used).
Honest sentences the docs carry: this does NOT raise `macos-app`'s
10.15 floor (Monterey users keep the AppKit app); macOS 12 users of
the SwiftUI variant get compile-time availability checking only —
runtime verification rides the D5:A owner moment (the runner never
runs the floor).

## 10. CI witness

- Non-required job `swiftui-frontend`, display name
  `macOS frontend (SwiftUI)` (distinct from `macOS frontend (Swift +
  AppKit)`), `runs-on: macos-latest`, steps: checkout →
  `cd macos-swiftui && swift build` → `cd macos-swiftui && swift test`.
  `swift build` alone does NOT compile test targets — `swift test`
  must ride the job or the golden tests are dead code.
- `swift-format` is omitted with a recorded reason: its availability
  on the runner image cannot be verified from this sandbox; an
  unverifiable step is theater. Formatting discipline = source review
  only.
- poll-checks.sh counts dynamically — after C2 the honest close
  wording is **"CI 13/13 green at <sha> (8 required + 5 preview)"**,
  read off the live check-runs of the NEWEST main SHA (stacked pushes
  cancel in-flight runs; a cancelled run on an older SHA reads as a
  failure — poll the newest SHA only).
- Failure policy: a red `swiftui-frontend` is evidence-then-fix
  forward and does NOT block tag cuts (it ships nothing — unlike
  `android-frontend`, which feeds the frozen asset path).

## 11. Local battery honesty

The dev sandbox has NO Swift toolchain and is Linux — a macOS SwiftUI
build is impossible locally regardless. Local verification = source
review + git hygiene ONLY. No record, comment, or doc may claim a
local build, test, or format of this tree. Zero dependencies → no
`--locked`-style discipline exists to import (no lock artifact).

## 12. Reuse + drift discipline

- COPY decision pinned: the three files are copies (adapted per rules
  3, 4, 8) into `macos-swiftui/`; the duplication-drift risk is
  recorded in the tree README with the canonical-copy rule: the
  AppKit tree's `Md3.swift` math is canonical for Swift ports.
- Color-math/seed drift rule: any change to `vdm-md3` color math or
  seeds patches Rust → Kotlin → BOTH Swift trees in the SAME commit,
  and extends `scripts/gen-md3-baseline.py`'s provenance sentence
  (today it names only the Kotlin port). The SwiftPM golden-literal
  test target is the Swift-side tripwire the chain otherwise lacks.
- The macos-app library+executable split (single source of truth via
  a shared library product) is recorded as the promotion-gate
  end-state, not this stage.

## 13. Records + deliverables

- C1: this spec + gate record 0013 + the dirty BACKUPS.md row + the
  duplicate-row dedupe (REL F1/F7).
- C2: `macos-swiftui/**` + additive ci.yml job + `.gitignore` lines +
  BUILDING.md section (single revertable unit; revert byte-restores
  main).
- C3 (close, grounded in implemented code — never pre-filled):
  parity-matrix SwiftUI columns §1.1–§1.5; the stale §1.2 Swift
  stale-rows cell refreshed; gap-register updates worded "closed in
  the S12-mac2 SwiftUI tree; macos-app still carries both gaps (its
  promotion owns them)"; `docs/DESIGN-MATERIAL-YOU.md` macOS-newer
  row flip + consumers note + the stale SSE line at :90 corrected;
  ROADMAP S12 row + brief flip; RUNBOOK §"SwiftUI preview (S12-mac2)"
  (two truths: zero local Swift verification — CI is the sole
  witness; red swiftui-frontend does NOT block tag cuts);
  `harness/MEMORY.md` session-19 entry; issue #16 close comment.
- C4: backup LAST, zero dirty files at manifest time.
- Recorded debt (NOT filled at activation): §2 packaging table and
  §3 owner-verification checklist have no SwiftUI rows — owed before
  the SwiftUI promotion gate is queued. No Dependabot coverage exists
  for SwiftPM trees (no `.github/dependabot.yml`) — noted honestly.
