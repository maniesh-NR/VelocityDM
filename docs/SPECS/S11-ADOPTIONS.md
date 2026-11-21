# S11 adoption specs — wave 1 (B1, B2, B9)

Behavior specs for the techniques adopted from the S11 reference-product
study (`docs/RESEARCH-IDM-STUDY.md`). **Provenance tier: T2** (static
reverse engineering of the reference build, 2026-11-21, Ghidra 12.2 /
binutils / direct JS read; fingerprint
`7346128f…d62ee`). **Clean-room barrier, restated per the standing S11
brief:** nothing in this file or its implementation is copied code, data
structures chosen for implementation reasons, resources, string tables,
or UI from the reference product. Every spec below is a functional
description in our own words; mechanism names, constants and thresholds
are chosen by us. Gate: `harness/ASSESSMENTS/0006-s11i-adoption-wave1.md`
(zero BLOCK; conditions folded into this text). These specs are
committed BEFORE the implementing commit (spec-first barrier); the
incorporation review diffs the implementation against this document.

---

## B1 — Cross-connection content audit with single-connection recovery

**Origin:** study §4 "Cross-segment integrity fallback" (T2). **Target:**
`vdm-core/src/engine/task.rs` (+ a snapshot surface field). **Goal:**
stability on servers/CDN edges that serve inconsistent bytes for the same
range across connections.

### Constants (ours)

| Name | Value | Meaning |
| --- | --- | --- |
| `AUDIT_BLOCK` | 256 KiB | audit anchor block size |
| `AUDIT_STRIDE` | 8 MiB | new completed bytes required between audits |
| `AUDIT_MAX_IN_FLIGHT` | 1 | concurrent audits per task |
| `AUDIT_RECOVERIES_MAX` | 2 | full-file recoveries allowed per task |

### Written-block ledger (in-memory only)

- Blocks are 256 KiB-aligned indices over the file (`block i` covers
  `[i*AUDIT_BLOCK, (i+1)*AUDIT_BLOCK-1]`, clamped to file size).
- A worker, upon fully completing a lease `[s, e]` (pos > e), inserts
  into the `written` set every block index whose block lies entirely
  within `[s, e]`. Blocks straddling a lease boundary become eligible
  when the neighboring lease completes them. No insertion on
  failure/pause releases (those bytes are not written).
- `audited` records blocks verified by a completed audit. The ledger
  tracks the two sets per task.
- **Determinism:** the next audit anchor is the LOWEST block index
  present in `written` and absent from `audited`. An inconclusive audit
  changes neither set, so its anchor stays the lowest and is retried at
  the next stride tick; late-written low blocks are still picked.
- The ledger is never persisted: **no `ResumeState` field may be added**
  (hard constraint — the Android `.vdmctl` reader is strict-schema). On
  task resume the ledger starts empty (fresh audits re-verify against
  disk bytes, which is exactly the comparison that matters).

### Cadence

- Audits run only while a segmented download is in the Downloading state
  with at least one live worker.
- The monitor fires an audit when: no audit is in flight AND at least
  `AUDIT_STRIDE` bytes of progress completed since the last audit START
  AND a `written`-but-not-`audited` block exists.
- Files smaller than `AUDIT_STRIDE` are never audited (overhead is
  disproportionate; single-connection fallback risk for tiny files is
  not worth it).
- Opt-out: `HttpOptions.no_audit: bool` (default `false`, additive,
  camelCase `noAudit` in RPC) disables audits for that task. Local
  policy knob only.

### Audit procedure (one anchor block, on a detached thread)

1. Pick the anchor block per the ledger rule. Mark in-flight.
2. **Read under the file mutex:** pread exactly the block's bytes from
   the `.part` file while holding the `Arc<Mutex<File>>` the workers
   write under (no torn reads; TOCTOU condition of the gate).
3. Fetch the same byte range over a **fresh TCP connection** (new
   `Conn::connect`; never reuse a worker connection) with a plain
   `Range: bytes=<start>-<end>` GET. When the task has more than one
   mirror, rotate the audit mirror choice deterministically so
   consecutive audits do not hit the same source (catches per-edge
   inconsistency; with one mirror the audit still catches
   per-connection inconsistency).
4. Audit traffic is diagnostic: it does NOT draw from the task/global
   token buckets (bounded: ≤ 256 KiB per 8 MiB of progress, one at a
   time).
5. Comparison streams the fetched body against the pread bytes.
   Byte-exact match → mark the block `audited`.
6. **Inconclusive audits** (connect/read error, non-206/200 status,
   short body, redirect weirdness): the block stays unaudited and the
   stride cursor does not advance; the task is never failed or degraded
   by an audit's own network trouble. The anchor is retried only at the
   next cadence tick.
7. **Mismatch:** any byte difference → record the block range, set the
   task's conflict flag, wake the monitor. The audit thread exits; the
   monitor owns recovery.

### Recovery (single-connection re-download)

- Rationale recorded per the gate: the audit samples 1/32 of the file,
  so one mismatch indicts the entire multi-connection-completed region —
  earlier unaudited blocks from inconsistent edges are equally suspect.
  Partial re-fetch from the mismatching block onward is therefore
  unsound; the honest response is a full re-download on ONE connection
  (the reference product's observed behavior, adopted for the stated
  sampling reason, not imitated blindly).
- Procedure: (1) if recoveries so far ≥ `AUDIT_RECOVERIES_MAX` (2), the
  task FAILS with our message "repeated cross-connection data mismatches
  after recovery retries" — no third loop; (2) otherwise increment the
  counter, set an internal reset flag checked by workers at their
  existing control-check points; workers return a Reset exit (releasing
  nothing — the pool is about to be reset); (3) once all workers are
  joined: reset the range pool to the FULL file, zero the done counter,
  clear the written/audited ledger, clear the reset flag, spawn exactly
  ONE worker; (4) snapshot updates: state stays `downloading`,
  `connections` = 1, `done` recomputes from the pool, and `notice`
  (new optional snapshot field, serialized `notice`, our static text)
  records the event; (5) the state file keeps saving normally (its
  remaining set now describes the full-file re-download; crash-resume
  is consistent because every byte below the recovery frontier is
  re-fetched).
- Recovery never changes user pause/stop semantics: the reset flag is
  engine-internal; pause/stop re-check points and their ~250 ms
  responsiveness are unchanged.

### Surfaces (ours)

- `TaskSnapshot.notice: Option<String>` — additive, `Option<String>`,
  set only by B1 recovery (text: "cross-connection data mismatch
  detected; restarting the file over a single connection"), cleared on
  task completion. Failure strings are fixed literals; the only dynamic
  content is the numeric block range.
- No new `Event` variants, no `ResumeState` changes, no RPC schema
  changes beyond the additive snapshot field.

### Tests owed (falsifiability, per gate)

- `TestServer` gains an "inconsistent edge" knob: a path whose served
  range bytes are altered deterministically after the Nth request for
  overlapping regions (config: e.g. `/inconsistent` serves correct bytes
  for the first fetch of each range and different bytes for any later
  fetch of the same range).
- e2e: engine downloads `/inconsistent/big.bin` with ≥2 connections →
  observe: notice set, exactly 1 live connection during recovery, task
  completes, output bytes byte-identical to the served FIRST-version
  content (the pre-corruption truth), ≥1 recovery occurred.
- Unit: ledger rules (lowest-index pick, boundary-straddling blocks,
  reset clears); audit cadence gating (stride/in-flight).
- Cap: the recovery entry point checks the counter before every reset
  (unit-level coverage via a directly driven helper + the e2e covering
  the single-recovery path).

---

## B2 — Optional connections (dynamic mid-download spawn/retire)

**Origin:** study §4 "Dynamic connection scaling" (T2). **Target:**
`task.rs` monitor loop + worker. **Goal:** throughput on fat pipes;
resource conservation on thin ones (retiring connections that stopped
helping).

### Eligibility (all must hold)

- The task is on the segmented path (size known, ranges accepted) —
  single-stream and HLS tasks never spawn optional connections.
- File size ≥ 16 MiB (setup amortization, Koo et al., existing
  `MIN_BYTES_PER_CONN` logic stays).
- **No rate limiting of any kind:** the task's `rate_limit_bps` AND the
  engine global bucket rate must both be 0. Under a limit, throughput is
  clamped by policy, not by connection count, and the spawn signal would
  be a false positive (gate condition).
- `opts.no_audit` is unrelated (audits and optional connections are
  independent features).

### Spawn rule

- Signals: aggregate `speed_ema` (existing EMA) vs `one_conn_capacity` =
  the highest per-mirror speed EMA (what the best single connection
  achieved).
- Spawn one optional worker when ALL hold: (a) ≥ 3 consecutive 1 s
  decision ticks with `speed_ema < 0.55 × live × one_conn_capacity`;
  (b) remaining bytes ≥ 8 MiB; (c) `live < cap`.
- Caps: `live ≤ min(32, size-based scale cap)` (existing absolute cap)
  and extra (optional) workers ≤ 8. Spawned workers inherit the
  connection stagger (wid-scaled sleep) so a SYN burst cannot be
  induced (server-blacklisting risk, gate condition).

### Retire rule

- Retire ONE optional worker (highest wid first) when either: (a) 3
  consecutive ticks with `speed_ema ≥ 0.85 × (live − 1) ×
  one_conn_capacity` (the extra connection stopped paying), or (b)
  `remaining / live < 4 MiB` (cannot amortize another connection — the
  Koo overhead argument, applied to retirement).
- Retirement is graceful: set the worker's retire flag; the worker
  finishes its CURRENT lease (bytes written, ledger updated) and exits
  with a Retired exit. The monitor counts it out of `live`.
- Base workers (the initial connection count) are never retired.

### Surfaces

- `TaskSnapshot.connections` reflects the live worker count as it
  changes (monitor updates on spawn/retire; 10 Hz progress cadence
  unchanged).
- No `ResumeState` persistence: dynamic workers are runtime state; a
  resumed task restarts with the configured base connections.
- Link-class presets (the study item's "presets" half) are deferred to
  wave 2 — wave 1 delivers the dynamic core only.

### Tests owed

- Unit: spawn/retire decision function (pure fn over the measured
  signals) — below-headroom persistence, caps, rate-limit guard.
- e2e (spawn-scenario witness): an aging test server (responses beyond
  the first requests for the path are degraded, globally — connection
  recycling is platform-dependent) with base `--conn 2` shows
  `connections` in the snapshot rising above 2 during the transfer and
  the file completing byte-identical.
- Bench witness: `bench-perf.sh` at base `--conn 2` records a
  throughput delta vs the pre-change binary (the A/B evidence), and the
  standard `--conn 8` run shows no regression. On the localhost bench
  the base connections saturate the pipe (no spawn triggers); the spawn
  demonstration is the e2e above, and the conn-2 bench documents
  no-regression plus the audit overhead.

---

## B9 — Assembly/file-lock conflict classification and retry

**Origin:** study §4 "Assembly/durability" (T2: the reference surfaces an
explicit "another application holds the file; reboot/resume assembles"
path). **Target:** `task.rs` failure sites + a pure classifier. **Goal:**
recoverability and honest messaging when AV/indexers/players hold the
output file.

### Classifier (pure, per-OS)

`fn is_lock_conflict(e: &std::io::Error) -> bool` — true for the OS error
codes that mean "another process holds the file":

| OS | raw codes | meaning |
| --- | --- | --- |
| Windows | 5 (ERROR_ACCESS_DENIED), 32 (ERROR_SHARING_VIOLATION), 33 (ERROR_LOCK_VIOLATION) | open/write/rename blocked by another holder |
| Unix | 1 (EPERM), 13 (EACCES), 16 (EBUSY) | permission/busy during file operations |

Classification only shapes MESSAGING and retry; it never suppresses an
error silently.

### Behavior

- **Finalize (the `.part` → output rename):** when the rename fails with
  a lock-class error, retry up to 3 times at 2 s intervals. If still
  locked, fail with (our words): "output file is locked by another
  application (for example antivirus or a media player); the download is
  complete — resume the task to retry saving it". Non-lock rename
  failures keep today's single-attempt behavior and message.
- **Resume-after-lock works by construction:** the `.vdmctl` is only
  deleted after a successful rename, so a locked-finalize task resumes
  into an empty remaining pool and re-attempts finalize directly (the
  existing code path — the spec requires this to stay true; an e2e-style
  unit check covers it by construction).
- **Write-path (open part / preallocate / positioned writes):** a
  lock-class failure uses the classified message "file is locked by
  another application; pause the task and resume when the lock is
  released" instead of the raw io string; non-lock errors keep today's
  messages.
- The `.part`/`.vdmctl` files, retry counts and all state machinery are
  unchanged.

### Tests owed

- Unit: the classifier against both OSes' code tables (cfg-gated arms
  compile-checked per platform in the CI matrix).
- Unit-level: finalize retry loop logic (bounded 3 × 2 s; lock vs
  non-lock branching) via a directly driven helper with an injected
  failing rename closure.

---

## Measurement plan (A/B evidence owed to the gate)

1. **Baseline run** on the pre-change release build (docs commits only
   in between — no code change): `bench-perf.sh` label `s11i-pre` at
   `--conn 8`, plus label `s11i-pre-conn2` at `--conn 2` (spawn
   scenario baseline).
2. **Post run** on the implementation build: same two labels suffixed
   `-post`.
3. Record: elapsed, throughput, `tcp_connections`, `http_requests`
   (audit overhead is visible here), byte-identical. The deltas land in
   the MEMORY close entry and the study's backlog rows get status
   notes.
4. The B1 stability demonstration is the inconsistent-edge e2e above,
   not a bench (it needs a lying server).
