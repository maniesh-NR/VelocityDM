# Assessment 0013 — S12-mac2 SwiftUI frontend (gate record)

**Date:** 2026-11-24 (UTC). **Stage under review:** S12-mac2 — a SwiftUI
variant of the macOS frontend (the "macOS newer" platform-matrix row),
the next item in the S12 queue per owner decision D3:D
(`harness/DECISIONS.md` round 1) after S12-lin2 GTK3 closed (gate 0012,
close commit 3f579ad). Standard frontend sub-stage — **no S11/T2
linkage, no T2 provenance, no CLEAN_ROOM disclosure row owed** (the
bench A/B bar binds study-derived engine changes; this stage touches
zero engine code). Proposed shape: a standalone SwiftPM tree
`macos-swiftui/` (sibling of `macos-app/`), SwiftUI `@main App`
lifecycle, Material You from the `Md3.swift` token-engine port, remote
JSON-RPC client of the shared core on 127.0.0.1:36990
(`docs/RPC.md`), SSE wiring per the gate-0010 behavior contract,
parity surface = `S12-PROMOTION-GATES.md` §1.1–§1.5 Swift column plus
the two registered Swift gaps (filename override, local `.torrent`)
closed as real UI. PREVIEW surface until its own promotion gate; ships
in NO release asset; `release.yml` untouched; the `v*` tag ruleset
(id 27298722) untouched; the 8 required checks untouched. New
NON-required CI job `swiftui-frontend` (macos-latest) = the only
compile/test witness (no Swift toolchain in the dev sandbox).

**Panel:** five-role per `PROTOCOL.md` (ATKR / FALS / REQ / INT /
REL), dispatched independently with the identical evidence pack (the
proposed shape + repository access + the verified environment facts:
no Swift toolchain in the sandbox → CI is the only compile witness;
macos-app is a single-executable SwiftPM package with zero
dependencies; `downloads.addTorrent` accepts `torrentPath` today so
the local-torrent gap is client-side-only; the current CI count is
12 jobs = 8 required + 4 preview). Agents saw no peer verdicts.

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | No structurally new attack surface (same loopback-only RPC surface, same URLSession transport class, zero-dependency SwiftPM posture already repo reality) — but ONE genuinely wider injection axis: SwiftUI `Text`'s `LocalizedStringKey` initializer (bound by string interpolation or literals) parses and renders Markdown, and on macOS 12+ Markdown links render as TAPPABLE Links — a hostile task name `[Invoices.pdf](https://evil.example)` becomes a phishing element inside a download manager. Pin `Text(verbatim:)` as the sole rendering sink for every daemon-relayed string (name, url, dir, outPath, error, notice, task-completed path, RPC/transport errors, banners); ban interpolation-into-`Text`, `Text` concatenation with daemon parts, `AttributedString(markdown:)`, and links from daemon strings; payload test in the CI job. Re-pin the FULL gate-0010 SSE contract for the new consumer (S12-SSE-WIRING is macos-app-scoped and binds nothing new): \n\n frame split with straddling, 1 MiB DROP cap (memory-exhaustion defense), lenient data:-only parse, terminal ⇒ reachable(false)+stale banner+frozen rows+2 s probe, 24 h timeouts, no Origin/Referer, delegate-queue threading. Trust-adjacent parsing must have a single source of truth or a drift tripwire. torrentPath: user-origin-only (picker/typed field), no silent fallback, error surfaced verbatim; the AppKit gap cells are NOT claimed closed. Transport hardening pins for the new tree: refuse redirects (willPerformHTTPRedirection → cancel), cookie-less dedicated sessions (httpShouldSetCookies=false, .never accept policy), hardcoded loopback addr (no user-configurable non-loopback in v1), ATS posture named in advance (loopback-scoped exception only if ever needed, never a global disable). CI job runs swift build AND swift test (a build-only witness cannot see the payload test). Md3.swift is an 18/27 token subset — honest row owed, "full parity" would be false advertising. |
| FALS | **PROCEED-WITH-CONDITIONS** | "Reuses the SAME token port" is half-true: Md3.swift imports AppKit (NSColor/CGColor bridges are AppKit/CoreGraphics); the math core is pure Foundation and constants-identical to `crates/vdm-md3` (verified constant-by-constant). Honest wording: "same math port, adapted bridges" — spec names which. **Md3.swift carries 18 of 27 Scheme roles** and its own header ("Full Material Design 3 color-role set") is already false advertising in the tree — the implementation commit corrects the header in BOTH copies and states the subset. RpcClient CANNOT be an identical copy under SwiftUI async callers: `nextId` is a plain var mutated on the caller's thread and completions hard-dispatch to main — concurrent `.task {}` callers race it; pin the @MainActor/main-isolated contract. Wire-shape trap: `downloads.addTorrent` params are TOP-LEVEL (`magnet`/`torrentPath`, `outDir`, `filePriorities`, `rateLimitBps`) — NOT nested under `options`; the AppKit client's `"options": [:]` key is silently ignored by the daemon, and a copied client mirroring that nesting would silently not apply a torrent limit. `filename` rides inside `downloads.add` options (HttpOptions, RPC.md:71-78). SseClient is Foundation-only and an identical copy is fine, BUT its stop() retain-cycle release is wired via applicationWillTerminate (main.swift:22-26) — the SwiftUI app must wire stop() into its termination path (@NSApplicationDelegateAdaptor or equivalent); the three files are a coupled unit (TaskSnapshot lives in RpcClient.swift) — copy is all-or-nothing. Deployment-target truth: @main App needs macOS 11+; .onSubmit/.task/.refreshable = 12+; NavigationStack/Grid/LabeledContent = 13+; the honest floor is 12 (13+ APIs forbidden or #available-guarded with degradation stated); macos-latest only availability-checks the floor, never runs it — runtime proof rides D5:A. Stale-rows contract must appear as framework-neutral BEHAVIOR (banner wording swap; frozen rows = no merges/re-lists while down; 2 s probe whose failures never mutate the model; recovery = probe success → stream reopen → full re-list precedes any merge; idempotent both directions incl. launch-with-dead-daemon; no orphaned stream at exit); SSE-spec §5.3's "(reason updated)" clause is NOT what the code does — implement or omit. Do not copy the AppKit dead no-op (refresh() fires a discarded settings.get on every successful re-list while the settings panel is closed). §1.2 Swift stale-rows cell still reads "~ (recorded gap)" — stale vs the register; C3 refreshes it. The AppKit add-field placeholder advertises .torrent paths its add path never kept — do not inherit the promise (the SwiftUI tree closes it for real). Close wording: CI 13/13 (8 required + 5 preview). |
| REQ | **PROCEED-WITH-CONDITIONS** | Queue position verified (PROMOTION-GATES §5.4, DECISIONS D3:D, ROADMAP S12 row, MEMORY session-18 tail). Wire facts verified true (addTorrent torrentPath RPC.md:59; add options filename :71-74; AppKit client exposes magnet only). **Code-reuse decision pinned to COPY; the macos-app library split is OUT-of-scope for S12-mac2** — macos-app is a built, previously gated tree (gate 0004 built it; gate 0010 owns its SSE files and recorded "no Package.swift change" as a non-goal); converting it would silently expand the stage from "additive new tree" to "modify a gated tree" and ride S12-mac2's revert unit. Any `macos-app/**` edit is explicit OUT (its own gated sub-stage if ever wanted); the duplication-drift risk is recorded honestly in the tree README. Pin exactly ONE deployment target with rationale + an explicit "does not raise macos-app's 10.15 floor" sentence. 18/27 token subset stated; 8-seed parity. Enumerate EVERY §1.1–§1.5 row the spec must cover: quality preset, speed limit, magnet, BT p/s line, per-task error AND notice lines (the copied TaskSnapshot has NO notice field — add `notice: String?`; gtk3 renders both), total-speed banner, empty state, Enter-to-add (.onSubmit), add-error surfacing, clamp ranges (connections 1..32 / max-active 1..16), scheduler stop window, shutdown-when-done, proxy structured round-trip, settings/limits/queue round-trips, seed picker + dark toggle persisted under the SAME UserDefaults keys `vdm.seed`/`vdm.dark`. Gap-register edits at close worded "closed in the S12-mac2 SwiftUI tree; macos-app still carries both gaps (its promotion owns them)". CI witness exact shape: job id `swiftui-frontend`, distinct display name "macOS frontend (SwiftUI)", macos-latest, swift build mandatory; swift test mandatory if a testTarget ships — and one SHOULD ship (pure-logic tests: frame split, parseBytes, RpcProxy.parse, seed cycling, payload sanitize — the only coverage this tree ever gets); swift-format: decide, don't hand-wave (omit with recorded reason if unverifiable); zero deps → no Package.resolved (state it). Absolute local-battery honesty: source review ONLY; no record may claim a local build/test/format. .gitignore gains /macos-swiftui/.build. Full gate-0012 deliverable set owed (matrix column at CLOSE grounded in code, DESIGN flip + consumers note, ROADMAP close-only flip, RUNBOOK section, tree README, BUILDING section, MEMORY entry, issue #16 comment, backup LAST zero dirty). §2 packaging + §3 owner-verification have NO SwiftUI rows — owed before the promotion gate is queued (recorded debt, not filled at activation). Commit split C1 gate+spec → C2 single revertable unit → C3 close → C4 backup LAST. |
| INT | **PROCEED-WITH-CONDITIONS** | CI precedent verified (swift-frontend ci.yml:135-143, bare swift build, no Xcode pin); the four preview jobs are consistent; the mirror job is additive-only. Non-required mechanics verified (check-run name = job name; "macOS frontend (SwiftUI)" ∉ the 8 required contexts; do not rename any of the 8). poll-checks.sh absorbs a 13th job with zero edits (dynamic count, 13 ≤ 100) — caveat: it treats `cancelled` as failure, and ci.yml's concurrency group cancels in-flight runs on stacked pushes, so ALWAYS poll the newest main SHA. Toolchain reality: swift-tools 5.9 needs Xcode 15+; macos-latest ships Xcode 16.x — proven by a stronger precedent (swift-frontend builds a .v10_15 package green against the macOS 15.x SDK; deployment target ≠ SDK is empirically true below 12/13). Real compile-time enforcement is @available checking: a 13-only API under a 12 target is a compile error the witness catches. Recommend pinning macOS 12; never silently 14+. **Swift-copy drift is tripped by NOTHING today** — the golden chain is Rust → md3-tokens CLI → gen-md3-baseline.py → Kotlin golden test only (no Swift leg); a silently edited Swift constant renders wrong colors with zero CI signal. Conditions: SwiftPM golden-literal testTarget (8 SEEDS × light/dark hardcoded hex from md3-tokens-baseline.json + at minimum primary/error roles) run via swift test — **load-bearing: swift build does NOT compile test targets; swift test must ride the job or the test target is dead code**; same-commit drift rule Rust → Kotlin → BOTH Swift trees for any color-math/seed change (extend gen-md3-baseline.py provenance text); library-split recorded as the promotion-gate end-state (single source of truth) or triggered by a third Swift consumer. .gitignore needs BOTH /macos-app/.build and /macos-swiftui/.build (no SwiftPM entries exist; no Package.resolved owed — zero third-party deps, hermetic CI). Local battery weaker than gtk3's: ZERO local Swift commands possible — source review + git hygiene is the entire local battery. Known macOS flake classes live in the required cargo job, not here; poll budget 2400 s absorbs macos-latest queue times. Required-job blast radius zero by construction (SwiftPM tree outside the Cargo workspace; root fmt/clippy/test structurally blind to it). |
| REL | **PROCEED-WITH-CONDITIONS** | release.yml builds zero Swift today (macOS assets = osxcross Rust egui+nmh only; publish mapping has no Swift case) — byte-untouched this stage; workflow_dispatch stays forbidden until promotion. Protection + ruleset verified LIVE via API: exactly 8 byte-exact required contexts, strict=false, enforce_admins=false, allow_force_pushes=false; ruleset 27298722 active (creation+update restricted on refs/tags/v*) — untouched by construction; the do-not-rename rule for the 8 display names is binding (any rename breaks protection matching). Rollback clean: C2 = macos-swiftui/** + additive ci.yml job + .gitignore lines; SwiftPM is not a Cargo workspace member, so root fmt/clippy/test are structurally blind; revert of C2 byte-restores main. CI count verified live: 12 check-runs on 4b2d9b4, all success → after the stage 13/13 (8 required + 5 preview); the close wording is read off live check-runs at the close SHA, never pre-written. **F1: harness/BACKUPS.md is dirty right now (the gate-0012 close row appended by the backup script is uncommitted) — the append MUST ride C1** so C4's manifest prints dirty files: 0. F2: .gitignore gains BOTH /macos-app/.build and /macos-swiftui/.build in C2. F3: pin the display name, ONE deployment target (not "12/13"), swift-tools-version, zero external Swift deps / no committed Package.resolved, token mechanism = Md3.swift copy. F4: DESIGN-MATERIAL-YOU.md:90 still says macos-app "polls at 200 ms (SSE wiring is a follow-up)" — stale since gate 0010; C3 corrects it or declares it out-of-scope explicitly. F7 (cosmetic, rides C1): BACKUPS.md has a byte-identical duplicated row (2026-11-15T02:30:13Z at lines 14 and 29). F8: the local battery is source review ONLY (zero Swift steps — weaker than gtk3's fmt+lockfile); no --locked analog exists (zero-dep package → no lock artifact); no .github/dependabot.yml exists → no Dependabot PRs will target the SwiftPM tree (note honestly, contrast gtk3's glib PR #17). §2/§3 SwiftUI rows are recorded debt, owed before that promotion gate is queued. |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** Conditions fold
into `docs/SPECS/S12-MAC2-SWIFTUI.md` and the execution plan below.

## Conditions → plan mapping

1. Rendering safety (ATKR #1) → spec rules: `Text(verbatim:)`-only for
   daemon-relayed strings via a single helper; interpolation/concat/
   AttributedString(markdown:)/daemon-string links banned; control-char
   sanitize helper with unit tests; payload case pinned.
2. SSE contract re-pin (ATKR #2 / FALS #6-7 / REQ #7) → spec restates
   the full gate-0010 consumer contract as its own rules (frame split,
   1 MiB drop cap, lenient parse, 24 h timeouts, no Origin/Referer,
   tick merge + metadata-drift dirty-mark, removals via task-removed →
   150 ms coalesced re-list, append-never-reorder, zero idle RPC) plus
   the framework-neutral stale-rows behavior set (banner, frozen rows,
   2 s probe, recovery, idempotency incl. launch-with-dead-daemon,
   teardown wired into the SwiftUI termination path); the §5.3
   "(reason updated)" clause is omitted (code truth).
3. Reuse + threading + wire shapes (ATKR #3 / FALS #1,3,4,5 / REQ #1 /
   INT #6,8) → COPY decision pinned (library split = promotion-gate
   end-state); Md3.swift math verbatim, bridges adapted to SwiftUI
   Color, 18/27 subset stated, false "full role set" header corrected
   in BOTH copies; RpcClient ADAPTED (@MainActor, + filename in add
   options, + torrentPath variant with TOP-LEVEL params, + notice in
   TaskSnapshot, redirect-refusal + cookie-less sessions); SseClient
   copied with stop() wired via @NSApplicationDelegateAdaptor; the
   dead settings.get no-op is not copied; same-commit drift rule for
   color math/seed changes.
4. Deployment pin (FALS #8 / INT #5 / REQ #2 / REL F3) → `.macOS(.v12)`
   pinned with the API rationale (App lifecycle 11+; .task/.onSubmit/
   .refreshable 12+; 13+ APIs forbidden unguarded); "does not raise
   macos-app's 10.15 floor" sentence; floor is compile-checked, not
   runtime-verified, until the D5:A owner moment.
5. Parity + honesty rows (FALS #2,9,10 / REQ #3,4,5,6) → spec
   enumerates every §1.1–§1.5 row incl. the two gap closures
   (filename override, local .torrent — real UI, no silent fallback);
   notice rendered; clamps honest; `vdm.seed`/`vdm.dark` keys;
   placeholder does not over-promise; SwiftUI matrix columns added at
   CLOSE grounded in code, never pre-filled; §1.2 Swift stale-rows
   cell refreshed at C3.
6. CI witness (ATKR #6 / REQ #8 / INT #1-4 / REL #4) → additive
   non-required `swiftui-frontend` job, display name "macOS frontend
   (SwiftUI)", macos-latest, swift build + swift test (golden
   testTarget + sanitize/frame-split/parse tests); swift-format
   omitted with recorded reason; poll newest SHA only; close wording
   "CI 13/13 green at <sha> (8 required + 5 preview)".
7. Hygiene + records (INT #9 / REQ #9,10,12 / REL #1-5, F1-F8) →
   .gitignore BOTH .build lines in C2; BACKUPS.md dirty row + duplicate
   row dedupe ride C1; BUILDING.md SwiftUI section + tree README in C2;
   C3 = DESIGN flip (+ the stale SSE line at :90 corrected), ROADMAP
   flip, RUNBOOK §"SwiftUI preview (S12-mac2)" (local = source review
   ONLY; red swiftui-frontend does NOT block tag cuts — ships nothing),
   MEMORY session-19 entry, issue #16 close comment; backup LAST with
   zero dirty files; §2/§3 SwiftUI rows recorded as debt.

## Execution plan

1. Commit C1 (docs): this gate record + `docs/SPECS/S12-MAC2-SWIFTUI.md`
   (spec-first, before any implementing commit) + the dirty BACKUPS.md
   row + the duplicate-row dedupe.
2. Commit C2 (implementation, the single revertable unit):
   `macos-swiftui/**` (Package.swift zero-dep .v12 + Sources + Tests +
   README), additive ci.yml `swiftui-frontend` job, additive .gitignore
   lines, docs/BUILDING.md SwiftUI section. Local battery: source
   review only.
3. Push main; poll the NEWEST SHA's check-runs; expect 13/13 (8
   required + 5 preview); flake = evidence-then-rerun.
4. Commit C3 (close): parity-matrix SwiftUI columns §1.1–§1.5 grounded
   in the implemented code (+ §1.2 Swift stale-rows cell refresh),
   gap-register updates ("closed in the SwiftUI tree; macos-app still
   carries both"), DESIGN-MATERIAL-YOU macOS-newer row flip (+ stale
   SSE line correction), ROADMAP S12-lin2/MAC2 "DONE (preview)" +
   brief update, RUNBOOK §"SwiftUI preview (S12-mac2)", MEMORY
   session-19 entry, issue #16 close comment.
5. Backup LAST (zero dirty files at manifest time).
