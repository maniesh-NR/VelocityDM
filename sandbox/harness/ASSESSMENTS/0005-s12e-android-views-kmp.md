# Assessment 0005 — S12-e Android Views + Kotlin Multiplatform (gate record)

**Date:** 2026-11-15 (UTC). **Change under review:** the S12-e
sub-stage execution plan — replacing the Compose UI with an Android
View-based Material 3 UI and restructuring the Kotlin code into a KMP
`:shared` module. **Panel:** the five-role panel per PROTOCOL.md
(ATKR / FALS / REQ / INT / REL), dispatched independently with the same
evidence pack (the drafted ROADMAP bullet + repository access). This is
the recorded sub-stage activation gate that ASSESSMENTS/0004 deferred
("each needs its own five-role gate before work").

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | R8/minify path never exercised by automation (assembleDebug-only) while S12-e adds reflection-sensitive surfaces (MDC, kotlinx.serialization, KMP); proguard keep-rule would not cover `:shared`; new deps unpinned + version catalog already drifting (coroutines 1.9.0 dead entry vs wired 1.8.1); `.vdmctl` resume path crosses an unvalidated trust boundary (external-files dir writable by other apps on API 26–28; unbounded intervals reach seek; stale-etag resume); filename/URL sanitization gaps ride along (substringAfterLast filename, `startsWith("http")` accepts `httpx://`); cleartext posture ambiguous (manifest attr + dead unreferenced network_security_config.xml); **the plan's seed claim was factually wrong — desktop DEFAULT_SEED is 0x4156A4, not 0x6750A4**; CI job action surface unspecified; empty `dm/` cruft |
| FALS | **PROCEED-WITH-CONDITIONS** | "single source of truth" misleading — a THIRD hand-maintained token copy (Rust, Swift-truncated, Kotlin) with no drift tripwire (no checked-in md3-tokens JSON; Swift has no test target); plan anchored to both Md3.swift's truncated API and Rust's full role set (conflicting); pre-12 static scheme would diverge from every desktop default (0x6750A4 vs 0x4156A4); Scheme does not cover all MDC theme attrs (mixed theme by omission — mapping table + visible-change note required); **jvmTest cannot compile as drafted (androidMain actuals invisible to the jvm target — jvmMain actuals required)**; expect/actual seam list materially incomplete (org.json, java.io.File, atomics, synchronized, Thread.sleep, Locale); **BrowserActivity.kt is a full Compose screen the plan never mentions, and the manifest's `.browser.BrowserActivity` entry is dangling (no such class exists)**; "wiring preserved" needs re-wording; release freeze is procedural (release.yml on a v1.0.x tag re-run would clobber frozen assets with the Views build); doc staleness list (android README, ARCHITECTURE, DESIGN row, root README wrapper/ABI lies) |
| REQ | **PROCEED-WITH-CONDITIONS** | Browser surface absent from the View tree (silent shrink + wrong preservation claim); add-dialog field parity (URL + filename override + speed limit + validation); card content/error-text/Remove-mapping/empty-state not enumerated; VIEW-intent auto-add not named; jvm actuals missing; commonMain JVM-API audit incomplete; root gradle plugins incomplete (no com.android.library / kotlin.multiplatform declarations; settings include); values-night themes.xml crash hazard under MDC; S8/S12-row wording overstates the Android leg's Rust-core status; "ships only via promotion gate" not mechanically true; soft verbs hardened to file lists |
| INT | **PROCEED-WITH-CONDITIONS** | **CRITICAL: jvm target cannot compile without jvmMain actuals**; **CRITICAL: expect/actual inventory 3 of ~8 real seams** (org.json/File/atomics/synchronized/Thread.sleep/Locale); **CRITICAL: Compose blast radius is 4 files incl. BrowserActivity.kt + dangling manifest entry**; root plugins + settings include + `.gitignore` (`android/shared/build/`) gaps; Gradle 8.9 = AGP 8.7 minimum with zero headroom, KMP-on-8.9 unproven — CI must pin byte-identical mechanism (ubuntu-22.04, temurin 17, setup-gradle 8.9) and be observed green before close; wrapper properties orphaned (no jar/scripts — complete or delete + fix doc lies); module config pins (namespace/compileSdk 34/minSdk 26/jvmTarget 17/coroutines 1.8.1/MDC 1.12.0/recyclerview 1.3.2, no `+`); local sandbox cannot build Android at all today (JRE-only 21, no SDK, no gradle — `tools/` gone this instance) → honesty condition: CI-only compile witness or provision locally; non-required job mechanics verified sound; release.yml inheritance latent gap = proguard keep for `:shared` |
| REL | **PROCEED-WITH-CONDITIONS** | **Promotion-gate leak is real and unenforced** — release.yml triggers on any `v*` tag and via workflow_dispatch, checks out the tagged/branch commit and clobbers assets; no tag protection rules exist (rulesets `[]`, tags/protection 404 — API-verified); RUNBOOK publish flow would clobber the frozen releases post-S12-e → push-main-only + tag ruleset required; RUNBOOK has zero Android coverage (failure policy, rollback, KMP) — section required; versionCode/versionName freeze unstated; commit shape/rollback (allow_force_pushes=false ⇒ revert-only) unspecified; close-out lifecycle omitted; jvm actuals + no-project-repos conditions; APK output path holds after `:shared` (rename step safe); debug-APK size delta direction favorable (observation for the promotion gate) |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** All conditions
were folded into the S12-e execution plan in `harness/ROADMAP.md`
(the "(a)–(e)" bullet) before the implementation commit; the
condition→plan mapping is itemized below.

## Conditions and how each was folded

1. **jvmMain actuals / jvmTest compilation** (FALS-4, REQ-5, INT-1,
   REL-8b) → plan (a): custom `jvmCommon` intermediate source set that
   both `androidMain` and `jvmMain` depend on; the moved SegmentEngine
   orchestration (OkHttp/RandomAccessFile/atomics/coroutines) lives
   there so every expect has an actual for both targets.
2. **Complete commonMain seam inventory** (FALS-5, REQ-6, INT-2) →
   plan (a): vdmctl schema → kotlinx-serialization 1.7.3; String
   paths (no `java.io.File`); interval/token-bucket/formatting as pure
   logic; orchestration (synchronized/atomics/OkHttp/RandomAccessFile)
   stays JVM-side in jvmCommon.
3. **Browser surface + dangling manifest entry** (FALS-6, REQ-1,
   INT-3) → plan (b): `browser/BrowserActivity.kt` rewritten as a REAL
   View-based Activity (toolbar + address field + sniffed-media list +
   WebView), finally matching the manifest entry.
4. **Seed correction** (ATKR-7, FALS-2) → plan (b): pre-12 static
   scheme seeded 0x4156A4 (`vdm_md3::DEFAULT_SEED`); the draft's
   0x6750A4 "desktop seed" label recorded as corrected by the gate.
5. **Drift tripwire instead of a third hand-copied literal set**
   (FALS-1, FALS-9) → plan (a): Kotlin goldens assert a checked-in
   `md3-tokens-baseline.json` generated from the Rust `md3-tokens`
   CLI; the Kotlin port mirrors the FULL Rust role set (not the
   truncated Md3.swift API).
6. **Scheme→MDC mapping + visible-change note** (FALS-3) → plan (b):
   documented mapping table (attrs left to MDC defaults named) +
   explicit pre-12 visible-change note.
7. **Full Compose parity enumeration** (REQ-2/3/4) → plan (b):
   three-field add dialog, card contents incl. error text + remove,
   empty state, VIEW-intent auto-add, Browser action.
8. **R8 coverage + keep rules** (ATKR-1, ATKR-2, INT-9) → plan (b)+(c):
   CI job runs `:app:assembleRelease`; proguard-rules.pro gains
   `-keep com.velocitydm.shared.**` + serialization rules.
9. **Version pinning + catalog activation** (ATKR-3, INT-6d) →
   plan (b): MDC 1.12.0 / recyclerview 1.3.2 / serialization 1.7.3 /
   coroutines 1.8.1 pinned through the activated catalog (dead
   Compose entries pruned).
10. **vdmctl validation + sanitization** (ATKR-4, ATKR-5) → plan (a):
    interval bounds/sum validation with restart-from-zero on violation;
    no-ETag resume refusal unless size matches; `sanitizeFilename()`
    + http/https-only URL validation on every add() path.
11. **Cleartext single-source-of-truth** (ATKR-6) → plan (b): manifest
    attribute carries unchanged; the unreferenced
    network_security_config.xml deleted.
12. **CI job byte-parity + runner** (ATKR-8, INT-5, INT-8) → plan (c):
    ubuntu-22.04, checkout@v4 + setup-java@v4 (temurin 17) +
    setup-gradle@v4 pinned 8.9, no new third-party actions, no secrets;
    branch protection re-verified at close-out.
13. **Gradle root/settings/gitignore** (REQ-7, INT-4) → plan (a)+(b):
    kotlin("multiplatform") + com.android.library apply-false;
    `include(":shared")`; `android/shared/build/` gitignored.
14. **themes.xml day/night + MDC parent** (REQ-8, INT-6e) → plan (b):
    one `values/themes.xml` on `Theme.Material3.DayNight`;
    values-night copy deleted; hardcoded windowBackground dropped.
15. **Release integrity / tag ruleset / push-main-only** (REL-1/2/3,
    FALS-10, REQ-10) → plan (d): GitHub ruleset on `v*` tags
    (restrict creation/update) added BEFORE the implementation merge;
    no publish script, no tag moves, no dispatch until the promotion
    gate; ROADMAP/DESIGN wording corrected (frozen ASSETS stay the
    shipped default; the tree loses the Compose app; a v1.0.x-tag
    re-run would attach the Views build).
16. **Versioning freeze** (REL-5) → plan (b): versionCode 1 /
    versionName 1.0.0 / applicationId unchanged; promotion gate owns
    bumps.
17. **Commit shape / rollback** (REL-6) → plan (d): one revertable
    implementation commit (android/** + additive ci.yml job only).
18. **Close-out lifecycle** (REL-7): this record + the ROADMAP plan are
    committed BEFORE implementation; MEMORY entry, ROADMAP status
    flip, BACKUPS.md line and `scripts/backup-sandbox.sh` close the
    stage (the S12-e backup snapshots uncommitted harness state, so a
    mid-work recycle cannot lose the gate record).
19. **Docs truth** (FALS-11, REQ-11, INT-5/10, REL-5) → plan (e):
    android README + ARCHITECTURE + DESIGN Android row + root README /
    BUILDING wrapper/ABI fixes; RUNBOOK "Android preview (S12-e)"
    section with the failure policy and rollback.
20. **S8/S12-row wording** (REQ-9) → stage table + S8 row now own the
    Android engine unification explicitly.

## Recorded (not fixed this session)

- Swift Md3 drift remains undetected (Md3.swift has no test target —
  the same JSON-tripwire approach is a natural S12-mac2/Swift-SSE
  condition).
- Local Android build provisioning is attempted this session
  (JDK 17 + Gradle 8.9 + SDK 34) into `tools/` (persistent); if
  provisioning fails, the first CI run on main is the compile witness
  (the 0004 pattern) and RUNBOOK records the recipe gap honestly.
- Runtime verification of the Views UI (MDC theme correctness,
  dynamic color on 12+) stays in the promotion criteria — owner on
  real hardware; the MDC-vs-raw-Views interpretation of the owner
  directive is flagged for the promotion checklist.
- GitHub-side settings (branch protection contexts, ruleset) are
  API-verified at close-out; any admin change after that is outside
  the repo's visibility.
- APK size delta (Compose out / MDC in) is a promotion-gate
  measurement, not an S12-e criterion.
