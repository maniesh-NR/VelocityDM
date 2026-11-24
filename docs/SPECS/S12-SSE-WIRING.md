# S12 — SSE wiring for the Swift macOS frontend (behavior spec)

**Status:** active spec for the S12 sub-stage queued next per D3:D/D5:A
(`docs/SPECS/S12-PROMOTION-GATES.md` §4 rows "Stale rows on daemon death"
and "SSE wiring (replace 200 ms poll)"; §5 step 1). Written BEFORE any
implementing commit (spec-first barrier). Gate record:
`harness/ASSESSMENTS/0010-s12-sse-wiring.md`.

**Surface:** `macos-app/` (Swift/AppKit frontend) only. No daemon, engine,
extension, RPC, or other frontend change. The protocol it consumes is
already committed and documented (`docs/RPC.md` §`GET /events`,
`crates/vdm-rpc/src/lib.rs` `sse_loop`/`event_payload`) — this spec adds a
consumer; it changes no wire behavior.

## 0. What the daemon sends (protocol facts this spec consumes)

- `GET /events` answers `200` with `Content-Type: text/event-stream`.
- Frames are `data: <single-line JSON>\n\n` (one `data:` field per frame,
  no `event:`/`id:`/`retry:` fields, **no heartbeats or comment pings**).
- Payload types: `task-added {id}`, `task-state {id,state}`,
  `task-progress {id}`, `task-completed {id,path}`, `task-failed {id,error}`,
  `task-removed {id}`, `queue-drained`, `config-changed`, and the periodic
  `tasks` tick `{type:"tasks", tasks:[TaskSnapshot…]}` emitted at most 10 Hz
  containing only snapshots whose (state, done, speed, error) changed.
- The server closes the stream when the client disconnects or the daemon
  exits; on loopback a daemon process death surfaces to the client as a
  transport error/EOF (connection reset or connection lost), because
  nothing else terminates the stream.
- RPC.md documents the consumer pattern: `task-progress` carries only the
  id, so consumers coalesce by re-reading `downloads.list` or by using the
  `tasks` tick's snapshots.

## 1. Connection model

1. The frontend holds at most one `GET /events` stream against the same
   loopback base URL as RPC (`127.0.0.1:36990` by default). The request is
   a plain URLSession data task: **no `Origin` and no `Referer` header is
   ever set** (rule 2 of the loopback trust boundary) and the `Host` names
   the loopback authority (rule 1) — the existing native-client posture,
   unchanged. An `Accept: text/event-stream` request header is sent.
2. The stream session uses request/resource timeouts of 24 h: the server
   legitimately sends nothing during fully idle periods (no heartbeats),
   so the URLSession default 60 s idle timeout would kill healthy streams.
   24 h bounds a wedged-but-unclosed socket instead of hanging forever;
   genuine daemon death is an error/EOF, not a timeout.
3. Only one reconnect path exists: when the stream reaches a terminal
   state (transport error, EOF, or non-200/non-`text/event-stream`
   response), the client enters fallback mode (§4); the first successful
   fallback probe re-opens the stream (§5). There is no tight reconnect
   loop.

## 2. Frame parsing

1. Bytes accumulate in a buffer; a frame is the buffer content up to the
   `\n\n` terminator (inclusive), which is then removed. Frames may
   straddle network reads; partial frames wait.
2. Within a frame, lines prefixed `data:` are joined (stripping one
   optional leading space) and the result must parse as a JSON object with
   a string `type`; anything else is ignored (counted, not fatal).
3. The buffer is capped at 1 MiB: on a frame-terminator-less buffer over
   the cap the buffer is dropped (a malformed or hostile stream cannot
   grow memory unboundedly). The daemon never sends such frames; the cap
   is defensive.

## 3. Event routing (the 200 ms poll is replaced)

1. **`tasks` tick → direct merge, no RPC.** Each snapshot in the tick
   replaces the row with the same `id`; unknown ids append (a `task-added`
   race cannot strand a row). After merging: recompute the total-speed
   banner, reload the table, update the empty-state label. Row order is
   list order of the current model, not reordered by ticks.
   The tick carries only snapshots whose (state, done, speed, error)
   changed, so a merge that changes a stored row **without any of those
   four changing** (metadata-only drift, e.g. resolved name/outPath)
   additionally marks dirty for the coalesced re-list of rule 2 — such
   changes still converge.
2. **Every other event type → dirty-mark + coalesced re-list.** A single
   pending re-list is scheduled 150 ms after the first dirty mark arrives;
   further marks within the window are absorbed. 150 ms is above the
   daemon's 10 Hz tick period (100 ms), so bursts collapse to at most ~7
   `downloads.list` calls per second of continuous event activity instead
   of the previous unconditional 5/s poll — the win is that an idle daemon
   now produces **zero** RPC traffic (the old poll fired forever).
3. **Initial state.** On launch the client performs one immediate
   `downloads.list` (today's behavior) and then opens the stream; on every
   (re)connect it performs one full re-list before trusting merges (§5),
   so a stream opened after downtime cannot apply ticks onto a stale
   model.
4. A re-list or merge runs only while the stream is considered connected
   (§4); events arriving after a terminal state are dropped (the fallback
   path owns refreshes from there).

## 4. Liveness and stale rows (gap-register fix)

1. Terminal stream state ⇒ `reachable = false`. The speed banner shows
   `daemon unreachable — rows stale` (with the transport reason, as
   today). The task list is **kept but explicitly frozen**: no merges, no
   re-lists apply until a probe succeeds, so rows cannot impersonate a
   live daemon. This is the honest minimum that fixes the registered gap
   ("stale rows on daemon death"): the previous behavior kept the banner
   but rows froze silently and continued to look authoritative.
2. When the daemon is reachable again, the banner returns to the
   speed/`idle` wording and rows resume updating (full re-list first).

## 5. Fallback probe (only while the stream is down)

1. While `reachable = false`, a `downloads.list` probe runs every 2 s.
   It doubles as liveness probe and recovery: on the first success the
   client flips to `reachable = true`, renders the fresh list, re-opens
   the stream, and falls back to event routing (§3).
2. While the stream is connected **no periodic poll runs at all** — the
   old unconditional 200 ms timer is removed, not augmented.
3. Probe failures during fallback keep the stale banner (reason updated);
   they never mutate the task model.

## 6. Threading

1. Stream delegate callbacks run on the URLSession's serial delegate
   queue; the frame buffer is touched only there (no locks needed).
2. Every UI mutation (banner, model, table reload) is dispatched to the
   main queue. Event handlers therefore never touch AppKit off-main.
3. Timers (`coalesce`, `fallback`) are created and fired on the main run
   loop only.
4. Lifecycle: `stop()` cancels the stream task and invalidates its
   session; it is wired into the app's existing termination path (or, if
   none exists for this programmatic AppKit app, the controller's tear-
   down) so shutdown never leaves an orphaned stream task.

## 7. Constants (chosen independently for this spec)

| Constant | Value | Rationale |
| --- | --- | --- |
| Coalesced re-list delay | 150 ms | > daemon tick period (100 ms) so bursts collapse; below human-perceptible latency |
| Fallback probe interval | 2 s | bounded recovery latency without idle-load |
| Stream request/resource timeout | 24 h | idle-safe (no heartbeats) but bounded |
| Frame buffer cap | 1 MiB | defensive memory bound; real frames are ≪ 64 KiB |
| Row-order policy | append unknown ids, never reorder ticks | stable UI; removals converge via re-list |

None of these values is derived from any reference product; they are
chosen for this frontend against the documented daemon cadence.

## 8. Non-goals (recorded, out of scope)

- No daemon/SSE protocol change: no heartbeats added, no new event types,
  no RPC method changes (the 24 h timeout, not a ping, covers idle).
- No row-visual "stale" dimming: the banner carries the state (the
  owner-verification checklist cites the banner row); row dimming can be
  a later cosmetic item if the owner asks.
- No persistence of stream state across app restarts; on launch the
  client always starts fresh (list → connect).
- No changes to the Win32/Slint/egui frontends' polling (their gates own
  that).
- No new permissions, no sandbox/entitlement changes, no Package.swift
  change (SwiftPM auto-includes the new source file).

## 9. Verification plan (honest boundaries)

1. **Wire truth:** the committed Rust SSE tests (`sse_stream_emits_events`
   + loopback Host/Origin refusal tests in `crates/vdm-rpc`) pin the
   frames this consumer must parse; additionally this session runs a
   live probe (`scripts/sse_probe.py`) against the built daemon and
   records the observed frames as the spec-§0 witness.
2. **Compile witness:** CI's `swift-frontend` job (`swift build` on
   macos-latest) compiles the change; the sandbox has no Swift toolchain
   (honest boundary), so local verification is source-review + wire
   probe only.
3. **Behavior witness:** the owner-verification moment (D5:A, after this
   sub-stage) runs the §3/§4 rows of the Swift checklist: add → progress
   flows without the 200 ms poll (idle ⇒ zero RPC), kill daemon → stale
   banner + frozen rows, restart daemon → auto-recovery within the probe
   interval. The agent cannot run AppKit (no displays); this is recorded,
   not silently assumed.
4. **No engine deltas ⇒ no bench A/B and no win7 port are owed** for this
   sub-stage; the vdm-rpc test suite is run as the wire-truth witness.
