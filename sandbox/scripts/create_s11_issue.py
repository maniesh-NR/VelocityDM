#!/usr/bin/env python3
"""Create the S11 roadmap issue on the v1.1.0 milestone (assessment 0003)."""
import json
import sys
import urllib.request
import urllib.error

TOK = open('/home/z/my-project/.gh_token').read().strip()
API = 'https://api.github.com/repos/maniesh-NR/VelocityDM/issues'

BODY = """## S11 — IDM competitive study (RE-assisted; owner-directed)

Study the reference product this project re-creates, looking for methods of **download management** and **browser integration** worth incorporating here for **performance, stability, and resource use**.

**Artifact (pinned, fail-closed):** `idman643build18.exe` — IDM 6.43 build 18, official URL <https://download.internetdownloadmanager.com/idman643build18.exe> — SHA-256 `7346128fab42957b4254eabdb012bfdbd751e9069361e98a6916f26bb03d62ee`, 12,164,608 bytes, PE32 i386. The binary is never committed or stored; any hash/size mismatch or fetch failure fails closed (re-pin only via the provenance log + official URL + stage-record note; the activation gate re-verifies against a fresh download).

**Method tiers**

- **T1 — black-box observation (default):** lawfully installed evaluation copy under observation — network traces (segment count/sizing, retry & refresh cadence, resume/auth, connection reuse), browser-integration IPC sniffing, resource monitoring (RSS, handles, startup).
- **T2 — static RE (conditional):** unpack the installer, identify the packer, decompile targeted routines (scheduler, segment allocator, refresh-link, site-login, browser marshalling). Requires a dated, owner-signed legal-basis clearance SEPARATE from the activation gate; a declined clearance escalates to the owner — never silently dropped.
- **T3 — dynamic RE (conditional):** instrumented runs in an isolated throwaway VM with API tracing; synthetic credentials only; traces never leave the VM.

**Clean-room hygiene (non-negotiable):** findings enter the repo only as own-words functional behavior specs; the spec is committed BEFORE the implementing commit; T2/T3 artifacts live only in a wiped throwaway directory outside the repo; every finding carries provenance (tier, tool+version, build SHA-256 prefix, date); no copying of code/resources/strings/UI.

**Deliverables:** `docs/RESEARCH-IDM-STUDY.md` (findings + improvement backlog mapped to engine areas and S4+), provenance log, `docs/CLEAN_ROOM.md` disclosure + README wording update, and a five-role gate before any incorporation. Each adopted technique must show a before/after `scripts/bench-perf.sh` A/B win with no test-battery or idle-RSS regression.

**Prerequisites (honest):** T1/T3 need a Windows-capable VM (owner-provisioned — this sandbox has none); T2 needs a PE RE toolchain (e.g. Ghidra). Until provisioned, the deliverable is the study plan + literature-level analysis — never invented measurements.

Advisory stage: runs parallel to S4+, never blocks the release train. Full brief: `harness/ROADMAP.md` (S11). Amendment review: `harness/ASSESSMENTS/0003-s11-roadmap-amendment.md`."""

payload = {
    'title': 'S11: IDM competitive study — RE-assisted review of download-management & browser-integration methods',
    'body': BODY,
    'milestone': 1,
    'labels': ['area:engine', 'area:extensions'],
}
req = urllib.request.Request(
    API,
    data=json.dumps(payload).encode(),
    method='POST',
    headers={
        'Authorization': f'Bearer {TOK}',
        'Accept': 'application/vnd.github+json',
        'Content-Type': 'application/json',
    },
)
try:
    with urllib.request.urlopen(req) as r:
        d = json.load(r)
        print('ISSUE CREATED #%s %s' % (d['number'], d['html_url']))
except urllib.error.HTTPError as e:
    print('HTTP %d: %s' % (e.code, e.read().decode()))
    sys.exit(1)
