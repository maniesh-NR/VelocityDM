# Assessment 0010 — S12 SSE wiring (Swift frontend, gate record)

**Date:** 2026-11-24 (UTC). **Change under review:** the S12 sub-stage
queued first per D3:D/D5:A (`docs/SPECS/S12-PROMOTION-GATES.md` §5 step 1;
§4 gap-register rows "Stale rows on daemon death" + "SSE wiring (replace
200 ms poll)") — replace the Swift frontend's unconditional 200 ms
`downloads.list` poll with consumption of the daemon's documented
`GET /events` stream, and make daemon death visibly stale the row list.
**Scope:** `macos-app/` only; no daemon/engine/extension/RPC change.
**Spec-first:** the behavior spec `docs/SPECS/S12-SSE-WIRING.md` was
written and the panel dispatched on it BEFORE any implementing commit.

**Panel:** five-role per PROTOCOL.md (ATKR / FALS / REQ / INT / REL),
independent dispatch, identical evidence pack: the staged spec + RPC.md
§`GET /events` + `crates/vdm-rpc/src/lib.rs` `sse_loop`/`event_payload` +
the current `RpcClient.swift`/`AppController.swift` + the gap register.

## Verdicts

| Agent | Verdict | Core findings |
| --- | --- | --- |
| ATKR | **PROCEED-WITH-CONDITIONS** | The stream request must keep the native-client posture: no `Origin`/`Referer` ever set (boundary rule 2) and a loopback `Host` (rule 1) — an `Accept: text/event-stream` header is outside the checked rule set and safe. The parser must cap its buffer (the spec's 1 MiB cap is accepted as the defensive bound) or a rogue loopback peer could grow memory via terminator-less data. Joining multiple `data:` lines must not concatenate JSON ambiguously — the daemon emits single-line payloads only, so a lenient join is acceptable but must stay lenient (ignore, never crash). No payload content reaches logs or error banners beyond the transport reason string. Verdict conditioned on the cap + no-log rules landing in code as specified. |
| FALS | **PROCEED-WITH-CONDITIONS** | Wire truth verified against `sse_loop`: frames are `data: <json>\n\n` (no `event:`/`id:`/heartbeats — the zero-length write used for disconnect detection sends no bytes, so clients genuinely see silence when idle); the `tasks` tick carries only changed (state, done, speed, error) snapshots and **omits removals entirely** — removal convergence must ride `task-removed` → re-list, which the spec does. One real falsifier: a snapshot can change fields the tick's change-detection ignores (resolved `name`/`outPath`) with none of the four tracked values changing — a pure tick-merge would then silently diverge from `downloads.list`. Condition: merges that alter a stored row without changing any of the four tracked values must mark dirty for a coalesced re-list. (Folded into spec §3.1.) The 200 ms-poll-removal claim is falsifiable only in runtime rows (D5:A) — the spec must keep claiming compile+witness honesty, not runtime proof the sandbox cannot produce. |
| REQ | **PROCEED-WITH-CONDITIONS** | Deliverable completeness: the sub-stage is defined by two gap-register rows plus its queue position — all three are covered (§4 fixes stale rows, §3 removes the poll, §5 precedes the owner-verification moment). Conditions: (1) `RpcClient.swift`'s header comment currently says the SSE tick is "optional … it polls at the same cadence" — it must be updated or the repo self-contradicts (honesty rule); (2) gap-register rows 157/158 flip at the CLOSE commit only, worded "implemented; compile witness = CI swift-frontend; runtime rows pending the D5:A moment" — never silently to done; (3) spec-first commit split (docs commit → implementation commit) is owed exactly as staged. |
| INT | **PROCEED-WITH-CONDITIONS** | Compile reality: the sandbox has no Swift toolchain, but CI's `swift-frontend` job (`swift build`, macos-latest) already exists and compiles this package — that is the compile witness for this sub-stage; the implementation must stay within macOS 10.15 APIs (URLSessionDataDelegate + Foundation only; no async/await, no macOS-12-only APIs) to keep the existing platform floor honest. Data-index arithmetic in the frame splitter is the top compile/panic risk — use whole-`Data` mutations (`range(of:)` + `removeSubrange` from the start index), never slice-typed indexes crossing mutation boundaries. Condition: `stop()` must be wired into the app's termination path (or the controller teardown) so no orphaned stream task survives exit. (Folded into spec §6.4.) The daemon build + `vdm-rpc` SSE suite run as the wire witness; no engine deltas ⇒ no bench A/B, no win7 port owed. |
| REL | **PROCEED-WITH-CONDITIONS** | Release posture unchanged: no tags, no release assets, v-ruleset untouched; `release.yml` needs no edit (the macos zip still stages from CI, and no asset content changes from a frontend-only source change until the Swift promotion gate). Conditions: (1) commit split — docs/gate/spec (+probe tooling) first, implementation second, harness close third, each pushed; (2) CI witness via `scripts/poll-checks.sh` **including the `swift-frontend` check** before the close commit; (3) MEMORY/ROADMAP/BACKUPS updated at close, backup script LAST, zero dirty files at backup time; (4) the probe transcript stays small and references only our own daemon's loopback output. |

## Consensus

Zero `BLOCK` → **the gate closes with this record.** Conditions were
folded into `docs/SPECS/S12-SSE-WIRING.md` (metadata-drift dirty-marking
§3.1; stop() lifecycle §6.4) and the execution plan below.

## Conditions → plan mapping

1. Buffer cap + lenient parse + no logging (ATKR) → spec §2.2/§2.3;
   implementation must not log payloads.
2. Metadata-only drift converges (FALS) → spec §3.1 second paragraph.
3. Removals converge via `task-removed` → re-list (FALS) → spec §3.2.
4. `RpcClient.swift` header comment updated (REQ) → implementation commit.
5. Gap-register flip wording + close-commit timing (REQ) → close commit.
6. macOS 10.15 API floor + whole-`Data` buffer ops + stop() wiring (INT)
   → implementation; CI `swift-frontend` = compile witness.
7. Commit split, CI witness incl. swift-frontend, MEMORY/ROADMAP/BACKUPS,
   backup last (REL) → execution plan.

## Execution plan

1. Wire witness BEFORE implementation: build the daemon, run
   `scripts/sse_probe.py` against a live download, record observed frames
   in the gate annex (confirms spec §0 against the running server).
2. Commit 1 (docs): this gate + spec + probe tooling + witness annex.
3. Commit 2 (implementation): `SseClient.swift` (new) +
   `AppController.swift` event-driven rewiring (merge/coalesce/fallback/
   stale banner) + `RpcClient.swift` header comment update. macOS 10.15
   APIs only.
4. Push commits 1–2; poll CI (11 checks incl. `swift-frontend`) via
   `scripts/poll-checks.sh`.
5. Commit 3 (close): MEMORY session entry, ROADMAP queue note,
   gap-register rows 157/158 flip (honest wording), BACKUPS.md entry.
6. Backup (LAST), wipe any throwaway state.

## Annex A — wire witness (probe transcript, 2026-11-24)

`scripts/sse_probe.py` against the freshly built daemon
(`target/debug/velocitydm`, loopback 127.0.0.1:36990) downloading a 6 MiB
file from a localhost HTTP server. Observed (full transcript:
`scripts/sse-probe-transcript.txt`): response head
`HTTP/1.1 200 OK / Content-Type: text/event-stream / Cache-Control:
no-cache / Connection: close`; frames `task-state` (probing → downloading
→ completed), `task-completed {id,path}`, one `tasks` tick carrying the
full camelCase TaskSnapshot object (matches the Swift `TaskSnapshot`
fields), `queue-drained`; every frame exactly `data: <single-line
JSON>\n\n`; no heartbeats. Spec §0 verified against the running server.
(Note: `task-added` did not appear before the first `task-state` on this
run — the observed set is a subset of §0's list; the consumer handles all
documented types generically, so the gap has no behavioral consequence.)
