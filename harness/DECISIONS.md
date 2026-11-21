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

Future rounds: the owner may answer by code ("D1:C, rest default");
unanswered items keep the menu's marked defaults. This ledger is the
source of truth after context loss; the chat menu is the interface.
