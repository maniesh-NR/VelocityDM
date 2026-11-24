# T1/T3 observation protocol (S11 — prepared per owner decision D4:B)

Status: PREPARED (2026-11-24), not yet executed. D4:B binds: the owner
provisions a Windows VM **later**; this protocol is the ready-to-execute
plan so both tiers can run the day the VM arrives. Neither tier is
silently dropped; T1/T3 execution remains gated on that VM plus the
standing S11 clearance rules (T2's clearance does not transfer — T1
needs the license-posture record at activation; T3 additionally needs
the dated owner clearance in the stage record).

Everything below is procedure, not observation: no studied binary is
executed, downloaded, or stored by this document's preparation.

## 0. Preconditions (checked at activation, fail-closed)

1. Owner-provisioned Windows VM exists and is reachable from this
   harness (or the owner runs the protocol on the VM and hands over
   the sanitized artifacts listed in §4).
2. The pinned artifact fingerprint re-verification (hash + size,
   fail-closed) is re-run at session start, per the standing re-pin
   policy (`harness/ROADMAP.md` S11 brief).
3. The license posture is recorded in the stage record BEFORE T1
   starts (product EULA RE-clause quoted, governing jurisdiction,
   owner authorization, the explicit "panel is not legal counsel"
   statement) — the T1 tier runs on that record; T3 additionally
   requires the dated owner clearance (same rule as T2).
4. The VM is a throwaway: no personal accounts, no real credentials,
   no persistent data; it is reverted/deleted after the observation
   window.

## 1. T1 — black-box observation (default tier)

Purpose: measure observable behavior; never inspect internals. The
studied product is installed from its official distribution URL only,
inside the VM, as a lawfully obtained evaluation copy.

### 1a. Download-engine measurement plan

Each scenario runs N times (N=3 minimum) against loopback/local test
servers we control (the same `scripts/testserver.py`-style fixtures
used by the engine battery, served from a second machine or the VM's
loopback), writing one CSV row per run:

| Scenario | What is measured |
| --- | --- |
| S-A small file, 8 conn cap | per-connection open count, range request sizes (segment count + sizing curve over time), total time |
| S-B large file (1 GiB synthetic), stable server | connection lifecycle (spawn/retire events over time), request size ladder, keep-alive reuse rate |
| S-C lossy server (throttled + random 5xx/reset) | retry cadence, retry-after honor, backoff shape, single-connection fallback trigger |
| S-D resume after kill | resume request shape (validators sent: If-Range/ETag/Last-Modified), partial-file naming/behavior, resume probe order |
| S-E expired link | failure classification timing, refresh/re-derive attempts (count + cadence), user-visible state |
| S-F auth'd download | auth handshake timing, cookie reuse, session refresh cadence |

Capture method: a local packet capture on the VM (loopback-visible),
filtered to the test servers' ports only; DNS and third-party traffic
are excluded from the capture (filter discipline recorded per run).
Measurements are aggregates: counts, sizes, timings, header NAMES and
shapes — never captured content payloads of third-party sites.

### 1b. Browser-integration channel observation

- The shipped browser extension (lawful install) is driven against a
  synthetic test page set; the NMH channel is observed by capturing
  the host process's stdin/stdout plumbing at the OS boundary
  (messages are ours to shape: synthetic download URLs only).
- Recorded: message types, ordering, size distribution, reconnect
  behavior, takeover decision timing.
- Browser telemetry/cloud endpoints are NOT contacted by the
  synthetic pages; the VM hosts file blocks vendor domains for the
  duration (protection recorded in the run log).

### 1c. Resource monitoring

RSS, handle count, GDI objects, thread count, startup-to-idle time,
and idle steady-state sampled every 2 s for the whole window
(perfmon counters + a small sampling script we own). Idle-RSS
methodology mirrors our own `scripts/bench-perf.sh` idle witness so
numbers are comparable.

### 1d. Deliverables

`docs/RESEARCH-IDM-STUDY.md` gains a T1 section with the same
provenance discipline as T2 (tier T1, tools+versions, date, findings
in our own words); the backlog may gain B-numbered candidates, each
still passing the five-role gate before any adoption.

## 2. T3 — dynamic RE (conditional tier; dated owner clearance required)

Only after T1, only with the clearance in the stage record:

- Instrumented runs in the same throwaway VM: API tracing limited to
  the process tree of the studied product (a user-mode tracer we
  bring; kernel drivers are NOT loaded).
- Trace scope: file/network/registry calls relevant to the §1a/1b
  scenarios; traces are filtered to the test-server endpoints and the
  product's own process — nothing else is captured.
- **Trace hygiene (hard rules)**: throwaway accounts and synthetic
  credentials only; trace files never leave the VM; only functional
  observations (the same aggregate shapes as T1) are transcribed out
  by hand; the VM and all traces are destroyed at the window's end
  (deletion verified in the stage record). Any accidental capture of
  real content or credentials ⇒ that trace is destroyed unread and
  the scenario is re-run.
- Deliverable: T3 section in the study doc, same provenance
  discipline, flagged for the stricter T2/T3 spec review.

## 3. Sequencing and budget

T1 (1 VM session): install → §1a/1b/1c scenarios → teardown ≈ one
working session. T3 (1 VM session): only the scenarios T1 left
ambiguous. Each ends with: artifacts sanitized, VM destroyed, stage
record + memory entry + backup.

## 4. What crosses the VM boundary (whitelist)

Aggregate CSVs (§1a), timing tables, message-type inventories (§1b),
resource samples (§1c), and our own notes. Nothing else: no captures,
no traces, no binaries, no logs, no screenshots of vendor UI beyond
what a finding sentence strictly requires (and never vendor
resources/strings verbatim beyond the minimum needed to identify a
behavior — clean-room discipline, `docs/CLEAN_ROOM.md`).
