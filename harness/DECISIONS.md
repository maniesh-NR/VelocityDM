# Decision ledger (owner menu answers)

The owner requested an in-chat decision menu so the rest of development
continues in parallel without blocking ("Give me a menu (within this chat)
to input any decisions I have to make, while the rest of the development
continues in parallel"). The menu was issued in chat (session 8,
2026-11-21) and mirrored in the session-8 MEMORY entry; this file is the
DURABLE record of answers and bindings — append-only, one round per
heading. Unanswered items run their marked defaults.

## Round 1 — answered in-chat 2026-11-21 (session 9): "d1:C, d2:B, d3:d, d4:b, d5:a, d7:B"

| ID | Decision (as the menu worded it) | Answer | Binding |
| --- | --- | --- | --- |
| D1 | S11-i wave-2 scope | **C** | Full wave 2 — B3 (HLS EXT-X-BYTERANGE), B4 (NM host resilience), B5 (cookie-jar forwarding), B6 (takeover UX), B7 (detector hardening), B8 (MIME→extension table) — PLUS the parked candidates **B13** (per-queue persisted scheduler state) and **B16** (browser-proxy propagation), plus the B2 link-class connection presets deferred to wave 2 by gate 0006. B10/B11/B12/B14/B15 stay parked (observe-first); N1–N6 stay rejected. |
| D2 | S4 (DHT BEP-5) activation timing | **B** | S4 activates NOW, **in parallel with wave 2** — its own five-role activation gate + spec-first behavior spec, then implementation as a parallel track. |
| D3 | S12 sub-stage order | **D** | **Promotion gates first** in the S12 queue: parity matrices, packaging plan, and the owner-verification protocol are prepared ahead of the SSE wiring and the S12-mac2 SwiftUI / S12-lin2 gtk3-rs / S12-lin3 Qt sub-stages. |
| D4 | T1/T3 (Windows VM tiers) | **B** | The owner provisions a Windows VM **later**; meanwhile the T1/T3 observation **protocol is prepared** (measurement plans, trace hygiene rules) so both tiers can execute the day the VM arrives. |
| D5 | Promotion gates (previews → shipped) | **A** | Owner runtime verification happens **after wave 2 closes + SSE wiring**, verifying a **full surface** at that point. |
| D6 | v1.1.0 milestone shape | *unanswered → default A* | Single v1.1.0 train; ordering via gates. |
| D7 | IDMan.exe bounded deep-dive | **B** | **Commissioned**: the bounded multi-session function-level decompile of the reference engine's scheduler and segment allocator, under the same T2 provenance + clean-room barriers (Ghidra toolchain re-provisioned this session — see MEMORY session 9). |

D3/D5 synthesis (recorded so the roadmap stays coherent): D3 orders the
S12 sub-stage QUEUE (promotion-gate preparation first, then SSE wiring,
then the remaining frontends); D5 fixes WHEN the owner's runtime
verification happens (after wave-2 close + SSE wiring). Neither
contradicts the other — preparation runs first, the verification moment
runs last.

## Round 2 — answered in-chat 2026-11-21: "D8:now"

The round-2 menu was issued in-chat in a round that was later lost to
context compression; its literal text was never committed (this entry
is written at the answer's arrival, from the durable queue state of
round 1 + gates 0007/0008). Honest reconstruction: after round 1 the
only commissioned-but-unscheduled item was the D7:B IDMan.exe bounded
deep-dive (scheduler + segment-allocator function-level decompile),
whose start timing was the open question; every other queue position
was already bound (wave 2 active per D1:C, S4 parallel per D2:B, S12
queue per D3:D/D5:A, T1/T3 protocol-prepared per D4:B).

| ID | Decision | Answer | Binding |
| --- | --- | --- | --- |
| D8 | D7 deep-dive start timing (the commissioned IDMan.exe scheduler/segment-allocator decompile) | **now** | The deep-dive starts IMMEDIATELY (session 10, 2026-11-21) as its own multi-session effort under the standing T2 barriers — fingerprint re-verified per the fail-closed policy at session start, toolchain re-provisioned per RUNBOOK — while the wave-2/S4 implementation queue continues unchanged as the parallel track. |

If the owner intended a different D8 semantics, this ledger is the
amendment point: append a correction row; the deep-dive work itself is
clean-room-bounded either way.

Future rounds: the owner may answer by code ("D1:C, rest default");
unanswered items keep the menu's marked defaults. This ledger is the
source of truth after context loss; the chat menu is the interface.

## Round 3 — answered in-chat 2026-11-24 (session 14): REA directive

The owner directed, verbatim: "Use reverse engineer anything
(https://github.com/morluto/rea) to decompile, reverse engineer and
analyse IDM (https://download.internetdownloadmanager.com/idman643build18.exe)
and incorporate any methods/techniques to improve performance and
stability of the download engine and browser integration."

| ID | Decision | Answer | Binding |
| --- | --- | --- | --- |
| D9 | Deep-dive session 2 tooling | **REA** | REA 6.10.1 (`rea-agents`, npm, MIT) joins the deep-dive toolchain as the structured evidence layer (PE import/resource/hardening inspection, JS application analysis of the shipped XPI, Ghidra-provider binding), alongside the established bounded Ghidra script pipeline — REA's native provider requires full auto-analysis, which still exceeds the sandbox budget on IDMan.exe, so the targeted decompile stays on the bounded scripts. Recorded honestly in the study Tools row. |
| D10 | Incorporation mandate | **now** | The directive authorizes adoption wave 3 from the session-2 findings, under the standing five-role gate + spec-first barrier: **B17** (ranged-request coding discipline + ranged-response invariants), **B18** (If-Range stale-resource fast path), **B19** (expired-link guidance, honest minimum). Gate record: `ASSESSMENTS/0009-s11i-adoption-wave3.md`; specs `docs/SPECS/S11-ADOPTIONS-WAVE3.md`. Parked items B10–B15 stay parked; observed-not-recommended rows unchanged. |

The round-3 directive also serves as the continued T2 clearance for the
deep-dive session 2 (same standing barriers; the pinned artifact was
re-verified byte-exact at session start per the fail-closed policy).

## Round 4 — answered in-chat 2026-11-24 (session 17): "proceed down the remaining adoption backlog with a fresh gate"

| ID | Decision | Answer | Binding |
| --- | --- | --- | --- |
| D11 | Wave-4 scope + gating | **fresh-gate adoption** | The remaining study backlog (§4d observations + parked candidates) proceeds through a NEW five-role gate (assessment 0011) — scope: B20 (header hygiene, narrowed by the panel), B21 (no-resume honesty + partial-state coherency + bounded Retry-After), B22 (TLS min-version knob), B23 (witness only), B24 (CocCoc NMH — deferred pending empirical path verification); B10–B15 stay parked; §4d leftovers recorded as N7/N8 + the FTP pre-condition. Spec-first barrier unchanged. |

Future rounds: the owner may answer by code ("D1:C, rest default");
unanswered items keep the menu's marked defaults. This ledger is the
source of truth after context loss; the chat menu is the interface.

## Round 5 — answered via structured in-chat menu 2026-11-27 (session 23): "D6:B; D5:A defer; T1/T3 defer; S10 defer"

The owner asked "Are there any decisions for me to make?" — the durable
state named four owner-gated items plus one never-answered menu code
(D6). All four were re-presented as a structured menu; the owner
answered every item. Nothing blocks the autonomous queue (S9 next,
then S8-W2); each answer below either defers an owner-gated item or
binds a release-train shape.

| ID | Decision | Answer | Binding |
| --- | --- | --- | --- |
| D6 | v1.1.0 milestone shape (re-presented; default A had been running since round 1) | **B** | **Split trains.** v1.1.0 carries the shared core + engine + extensions (everything promotable on CI evidence alone); the **per-surface native-frontend promotions** (Win32/D2D, Slint, Swift/AppKit, SwiftUI, GTK3, Qt, Android Views) land in a separate **v1.2.0 train**, each still gated on the owner's D5:A runtime verification. The GitHub `v1.1.0 - roadmap` milestone stays the tracking device for roadmap issues; the split binds release/packaging planning (release.yml trains, CHANGELOG grouping, tag sequencing), not issue tracking. |
| D5x | D5:A owner runtime-verification timing (the unblocked moment from round 1) | **defer** | The owner defers hands-on verification. Per-surface promotions stay gated on the owner exactly as bound in round 1; **no CI-only substitution is authorized** (the promotion-gate policy is unchanged); the autonomous queue continues meanwhile (S9, then S8-W2). |
| D4x | T1/T3 Windows-VM provisioning timing | **defer (re-affirms D4:B)** | The prepared observation protocol (`docs/T1-T3-OBSERVATION-PROTOCOL.md`) keeps waiting for an owner-provisioned VM; T1/T3 remain the only open S11 study items; no agent-side substitute is commissioned. |
| S10x | S10 Flathub submission timing | **defer** | S10 stays PENDING — an owner-owned verified Flathub account is a hard precondition and remains un-provisioned; the queue keeps S9 + S8-W2 ahead of it. |

Future rounds: the owner may answer by code ("D1:C, rest default");
unanswered items keep the menu's marked defaults. This ledger is the
source of truth after context loss; the chat menu is the interface.
