# S11 research: IDM reference-product study (T2 static RE)

Study question (owner-directed, roadmap S11): does the reference product —
Internet Download Manager 6.43 build 18 — contain methods of download
management or browser integration worth adopting in VelocityDM for
performance, stability, and resource use?

## Provenance and authorization

| Item | Record |
| --- | --- |
| Artifact | `idman643build18.exe`, official URL `https://download.internetdownloadmanager.com/idman643build18.exe` |
| Fingerprint | SHA-256 `7346128fab42957b4254eabdb012bfdbd751e9069361e98a6916f26bb03d62ee`, 12,164,608 bytes, PE32 i386 (GUI), 5 sections |
| Re-verification (activation gate) | Fresh HTTPS download from the official URL on 2026-11-21; size and SHA-256 matched the pinned fingerprint byte-exact before any work started (fail-closed policy honored; no re-pin needed) |
| Tier | T2 — static reverse engineering (decompile/disassemble; no execution of any studied binary; this sandbox has no Windows anyway) |
| Owner T2 clearance | Owner directive received in-chat on 2026-11-21, verbatim: "Use Ghidra CLI analyzeHeadless to decompile and reverse engineer the IDM install file (https://download.internetdownloadmanager.com/idman643build18.exe) to study if there are any methods of download management or browser integration that can be incorporated in to this project to improve performance, stability, and resource use". Recorded as the T2 clearance the S11 brief requires (separate from and after the 0003 amendment gate). Disclaimer per protocol: the panel is not legal counsel; posture is owner-authorized static analysis of a lawfully obtained copy for interoperability study. |
| Tools | Ghidra 12.2.2 `analyzeHeadless` (+ Temurin JDK 25; zip hash-verified per RUNBOOK), binutils `objdump`/`strings`, Python `zlib`/`zipfile` for container unpacking; deep-dive session 2 (2026-11-24) adds **REA 6.10.1** (`rea-agents`) as the structured evidence layer (PE import/resource/hardening inspection, the JavaScript application analyzer on the Firefox XPI, and a Ghidra-provider overview of the installer stub) — the targeted bounded decompile stays on the established no-analysis Ghidra script pipeline, because REA's native provider requires full auto-analysis, which still exceeds this sandbox's budget on the 2.97 MB code image |
| Fully decompiled components | installer stub (272 functions, all decompiled), `IDMMsgHost.exe` (83 functions, all decompiled), `MediumILStart.exe` (172 functions, all decompiled) |
| Bounded-to-strings components | `IDMan.exe` (2.97 MB of code — full Ghidra decompile exceeds this sandbox's CPU/RAM/time budget; analyzed via imports, strings, RTTI class names, version resources, **then the D7/D8 function-level deep-dive of its scheduler/connection-scaling machinery — see §4b**), 16 integration helper binaries (imports + strings), all three Firefox XPI generations and the Chrome CRX (JavaScript read in full) |
| Deep-dive provenance (D7:B, D8:now) | Function-level decompile of the reference engine's scheduler + connection scaling, session 1 of a bounded multi-session effort, 2026-11-21: same pinned artifact (fail-closed re-verified byte-exact at session start), same throwaway-dir discipline (all decompiler output wiped after distillation), Ghidra 12.2 no-analysis import + anchor-string/backward-start + RTTI-chain scripts + bounded targeted decompilation (184 functions selected by evidence; helper scripts backed up with the sandbox toolbox); findings below are functional descriptions in our own words — no code, structures, or constants copied for implementation use |
| Deep-dive provenance (session 2, 2026-11-24) | The commissioned deep-dive continued on its session-2 targets (worker range arithmetic, queue-state writer, site-login/refresh-link) under the same barriers: pinned artifact re-downloaded from the official URL and verified byte-exact before any work (fail-closed honored, no re-pin); Ghidra 12.2 + Temurin JDK 25 re-provisioned per RUNBOOK; REA 6.10.1 installed and bound to the Ghidra provider for structured evidence; bounded targeted decompilation (152 functions incl. 10/10 session-2 anchor functions and the header-range builders + If-Range site), all output confined to the wiped throwaway dir; findings in §4c below are functional descriptions in our own words |
| T2 artifacts | All raw material (installer binary, extracted payloads, decompiler output) lived only in a throwaway directory outside the repo and was wiped after distillation; the binary was never committed, stored in the repo, or backed up |

Clean-room barrier (unchanged, restated): nothing below is copied code,
data, resources, string tables, or UI. Everything is a functional
description in our own words at the level of behavior and mechanism.
Any adoption of a technique still requires the standing S11 gates:
behavior spec committed BEFORE the implementing commit, a five-role
panel, an A/B win via `scripts/bench-perf.sh` (performance, stability,
or resource use) with no regressions in the existing batteries.

## 1. Packaging and installation method

The distribution file is a small self-contained PE32 stub (~47 KB of
code) with ~12 MB appended in a **proprietary zlib container**:

- A literal marker `!--END FILE--!` sits in the last 20 KB of the file;
  the 4 bytes before it hold the file-table offset.
- Entries chain as `[u32 uncompressed size][u32 compressed length]
  [3×FILETIME timestamps][zlib stream]` (zlib 1.1.4-era inflate is
  linked into the stub, with the classic zlib diagnostic strings).
- Entry 0 is a plain-text **manifest** that drives the whole install:
  destination files (`P1..P127`), Start-menu shortcut list (`LNK` with
  display names), and a `<REGISTRY>` section of values to write
  (uninstall keys: DisplayIcon/Publisher/URLInfoAbout/HelpLink).
- The stub checks ~10 MB free on the temp disk (falls back to a
  folder-picker dialog), extracts to `%TEMP%\IDM_Setup_Temp\IDM<n>.tmp`
  restoring the recorded file timestamps, then `CreateProcessW`s the
  extracted setup with `-d "<dir>"` (plus `-rmsetup` on remove flows;
  `-skipdlgs`/`-delsetup` CLI switches exist).
- Single-instance via a named mutex; Windows 7+ version gate; an
  AppCompat registry key write and `IsWow64Process` awareness; the
  manifest demands `requireAdministrator`.

The extracted setup additionally writes the legacy Firefox extension
**directly into `...\Mozilla\Firefox\Profiles\%s\` directories** and
installs + starts the kernel-integration driver as a Windows service
(`OpenServiceW`/`StartServiceW`).

*Assessment for VelocityDM:* observed, **not recommended for adoption**.
Our Inno Setup `velocitydm.iss` is more featureful (uninstall registry,
localized pages) and the maintenance cost of a hand-rolled container is
not worth ~1 MB of size. The timestamp-restore and manifest-driven
ideas are noted but low-value here.

## 2. Process and integration architecture (what ships)

The payload manifest names 127 files. The integration-relevant ones
form a layered architecture:

| Layer | Components (from the manifest) | Role |
| --- | --- | --- |
| Main engine | `IDMan.exe` | MFC GUI + engine; **no static network imports — WININET and WS2_32 are `LoadLibrary`-ed at runtime** |
| Browser ↔ engine link (modern) | `IDMMsgHost.exe` + `IDMMsgHost.json` (`com.tonec.idm`, stdio, two `chrome-extension://` origins) + `IDMGCExt.crx` (Chrome MV3) + `idmmzcc3.xpi` (Firefox MV2 WebExtension) | native-messaging bridge, extensions |
| Browser ↔ engine link (legacy Firefox) | `idmmzcc.xpi`, `idmmzcc2.xpi` (XUL) with `idmmzcc7.dll`/`idmfsa.dll` XPCOM components (agent interfaces `IIDMEFSAgent` generations 1–8, Gecko `NS_CString*` glue) | in-process XPCOM components |
| Browser ↔ engine link (IE) | `IDMIECC.dll`/`IDMIECC64.dll` (WinInet+ws2_32+urlmon), `IEMonitor.exe` (hooks `BeforeNavigate2`, IE `Ext\Settings` registry), `idmbrbtn.dll`/64 | IE-era monitoring |
| COM surface | `idmantypeinfo.tlb`, `IDMan.CIDMLinkTransmitter` | in-proc/out-of-proc COM object for handing links to the engine |
| Kernel network layer ("advanced browser integration") | `idmwfp32/64.sys` (WFP callout driver), `idmtdi32/64.sys` (TDI driver), `.inf` + signed `.cat`, installed+started as a service; `IDMNetMon.dll`/64 (WS2_32 + **linked zlib inflate — decompresses intercepted gzip/deflate bodies on the fly**) | connection-level interception independent of extensions |
| Privilege/separation | `MediumILStart.exe` (30-line program: `CoCreateInstance` CLSCTX_LOCAL_SERVER on a private CLSID `ac746233-…`, 1 s retry — the COM de-elevation trick to relaunch the UI at medium integrity), `idmBroker.exe` (per-user typelib/registry registration via `RegisterTypeLibForUser`/`RegDeleteKeyExW`) | elevation hygiene |
| Helpers | `idmvs.dll` + `IDMVMPrsAA.dll` (ARM64) video-stream parsers; `idmindex.dll` (embeds SQLite); `idmftype.dll`/`IDMFType.dat` file-type detection; `IDMGetAll.dll` ("Download All Links With IDM", `InternetGetCookieExW`); `downlWithIDM.dll`; `IDMShellExt(.64).dll`; `IDMGrHlp.exe`; `idmcchandler7.dll` | parsers, shell, grabber |

Decompiled `IDMMsgHost.exe` mechanics (all 83 functions):

- Cold-start: opens `Local\IDMEventMonitor` mutex; if absent, reads
  HKLM `Software\Internet Download Manager\InstallStatus` (installed
  states 0/3) and **launches the manager via COM** ("Launched manager
  via COM"), then continues.
- Session scoping: `ProcessIdToSessionId` — the pipe name is
  `\\.\pipe\IDMNetworkMonitor.<sessionId>`; failure to resolve the
  session is fatal ("Cannot determine session Id").
- Bridge: `GetStdHandle` stdio ↔ the named pipe with **overlapped
  asynchronous I/O**, separate input/output threads, events and
  semaphores, and 1 MB / 64 KB ring buffers.
- **Peer hardening**: resolves `GetNamedPipeServerProcessId`/
  `GetNamedPipeClientProcessId` (+ PSAPI
  `GetProcessImageFileNameW`, `NtQueryInformationProcess`) and keeps a
  `\cmd.exe` check — the host verifies who is on the other end of the
  pipe and refuses shells as relays.

This is a **thin, hardening-focused relay**: the NM host contains no
download logic at all — the protocol semantics live in the extension
and the engine's network-monitor side (an `idmcmd://startnetmon/`
command string exists for monitor startup).

## 3. Browser-integration methods (modern generation)

Read in full from `IDMGCExt.crx` (Chrome MV3) and `idmmzcc3.xpi`
(Firefox MV2) — both named "IDM Integration Module" 6.43.1:

**Permissions posture.** MV3 Chrome: `scripting, tabs, cookies,
contextMenus, webNavigation, webRequest, declarativeNetRequest,
downloads, downloads.shelf, downloads.ui, management, storage, proxy,
nativeMessaging` + `<all_urls>` host permissions, content script at
`document_start` in all frames, `externally_connectable` limited to
their own sites for the welcome page. Firefox MV2 adds
`webRequestBlocking` + `privacy` + `idle` and matches `ftp://`/`file://`
too; CSP pins the background script by SHA-256 hash.

**Interception paths (background).** Two parallel mechanisms:

1. *Downloads pipeline*: `downloads.onCreated` +
   `downloads.onDeterminingFilename` (filename decision point), shelf
   suppression via `setUiOptions`/`setShelfEnabled`; when the engine
   declines, the browser download is `downloads.resume`d — i.e.
   **cancel-then-resume handoff** instead of leaving broken downloads.
2. *webRequest observation*: `onBeforeRequest` (two registrations with
   different URL sets; one with `requestBody`), `onBeforeSendHeaders`
   with `extraHeaders` (to see Cookie/Referer/User-Agent),
   `onHeadersReceived` with `responseHeaders`, `onResponseStarted`,
   `onErrorOccurred`; decisions use the header watchlist
   `date, content-type, content-length` plus `Content-Disposition`.
3. *DNR session rules*: `declarativeNetRequest.updateSessionRules`
   builds `regexFilter` + `resourceTypes:["main_frame"]` rules
   dynamically (add/remove by rule id) — e.g. redirect/cancel a
   main-frame navigation into the takeover flow; one branch uses the
   `redirectUrl:"javascript:void(0)"` trick for suppressed frames.

**Cookie/session forwarding.** `cookies.getAll({url, session:false})`
(plus `firstPartyDomain` on Firefox), serialized `name=value; …` —
the full cookie jar for the URL travels to the host. Also reads
`storage.local` (version, "silent", last-update), and detects
`isAllowedIncognitoAccess`.

**NM host link.** `browser.runtime.connectNative("com.tonec.idm")`
persistent port with `onDisconnect` + **1500 ms re-connect timer**;
host-absence strings ("messaging host not found|no such native
application|application is not installed") are matched for graceful
degradation (keep the browser working when the host is missing).

**Page-side detection (content + MAIN-world scripts).**

- A `document.js` is injected into the page's main world. It hooks
  page `fetch` and the response-body readers (json/text/arrayBuffer/
  blob/clone, XHR accessors), tagging requests with marker suffixes
  (`.pxr-<n>` / `.pfm-<n>` style) so the extension's webRequest layer
  can **re-associate and re-read the page's own XHR/fetch bodies**;
  hooks are installed/uninstalled on demand and bridged to the
  isolated-world content script via `postMessage` with numeric
  protocol IDs, plus a generic **page-object query language**
  (`$`, `<`, `>`, `=` operators with N-levels-up parent traversal and
  dotted path resolution — e.g. resolving
  `*.return.return.memoizedProps.coreVideoPlayerMetaData.videoFBID`
  walks React Fiber internals to find Facebook player video IDs).
- The isolated content script watches `video/audio/object/embed`
  elements: element geometry gating (a video counts only when it is
  ≥15 px wide, ≥10 px tall, `videoWidth`/`networkState==2` while
  `readyState<1`, and must be `document.contains`-attached),
  transparent-overlay detection (computed `rgba(...,0)` match),
  `MutationObserver` (title childList, element `style` attribute) +
  `ResizeObserver`, WeakRef bridges to DOM nodes with `__idm_uri__`
  markers, inline-`<script>` text scanning for embedded media URLs,
  modifier-key tracking (Shift/Ctrl/Alt/Insert/Delete) for
  click-plus-modifier takeover, browser-variant branching (Edge/Opera/
  Yandex UA strings), site-specific selector packs (Facebook class
  names, Vimeo `player.vimeo.com/video/(\d+)`, `div.vp-video`/
  `div.vp-telecine`).
- A `captured.html` web-accessible page with a hash fragment
  (`captured.html#<url>`) reports a takeover and immediately
  `history.back()`s — a clean in-tab confirmation without navigation
  side effects; also used for the extension↔site registration flow.
- A MIME→extension normalization table (≈100 entries:
  `video/mp2t→TS|M3U8`, `video/f4m→F4M`, `application/x-subrip→SRT`,
  …) for consistent filename choices.

Legacy generations for contrast: XUL XPCOM components with direct
window-mediator DOM access (`nsIWindowMediator`→`content.document`),
exported native helpers like `IDMMzCC_DownloadLastFLVwithIDM` /
`DownloadLast10FLVwithIDM`; IE side hooks `BeforeNavigate2` via
`IEMonitor.exe` with `Ext\Settings` registry integration.

**Kernel layer.** WFP/TDI callout drivers + `IDMNetMon` with inline
zlib inflate — the toggle the product calls "Use advanced browser
integration". It observes connections and decompresses bodies at the
network layer, catching downloads that extension-only integration
misses (the product's own troubleshooting text admits rare conflicts
with 3rd-party network software, with a one-click disable).

*Assessment:* this is the most directly transferable part for
VelocityDM (we ship MV3 extensions + an NM host today). See backlog
items B1–B9.

## 4. Download-engine methods (IDMan.exe, bounded analysis)

From imports/strings/RTTI (full decompile was out of sandbox budget):

- **HTTP/2 support**: error strings for SETTINGS/PRIORITY_UPDATE
  frames, "max concurrent streams exceeded", server-push denial — a
  modern multiplexed stack with priorities, not just HTTP/1.1.
- **Dynamic connection scaling**: strings show live lifecycle
  messages — "Need optional connections", "Optional connections,
  connecting...", "have been created", "not needed" — segments spawn
  and retire **optional connections mid-download**; connection-count
  presets are offered per link class (high: Ethernet/4G; medium:
  ADSL/3G; low: dial-up/EDGE) with honest guidance that high
  connection counts can get a client blacklisted by servers.
- **Cross-segment integrity fallback**: "IDM started to download the
  file using one connection because of data corruption when newly
  received data in several connections did not match the same data
  downloaded earlier" — received ranges are **cross-verified**; on
  mismatch (caches/CDN nodes serving inconsistent bytes per range) the
  engine automatically re-downloads single-connection.
- **Resume/refresh**: full conditional-request set (`If-Range`,
  `If-None-Match`, `If-Modified-Since`, `ETag`, `Last-Modified`,
  `Content-Range`), plus a `&bytes=<start>-<end>` URL query fallback
  for servers that ignore `Range:` headers.
- **Auth/site login**: Basic and Digest (full construction string),
  cookie replay, site-login dialogs with form completion; FTP with
  `MDTM` timestamps, multi-connection negotiation awareness ("FTP
  server doesn't allow connections"), command-pipelining detection.
- **Adaptive media**: HLS `#EXT-X-BYTERANGE:` fragment handling,
  `ts2mp4` remuxing (AAC/AC-3 codec classes) — TS segments are
  re-assembled into `.mp4` output on the fly (`%s.mp4.tmp`);
  Anvato-CDN `#ANVATO-SEGMENT-INFO:` segment-info parsing.
- **Queue/scheduler**: multiple named queues (`CDownloadQueue`,
  `CQueuesArray`, `IDMSTRUCT_QUEUEITEM`), per-queue persisted
  scheduler state (`%s\Scheduler\q_%s.dt`, `s_%s.dt`), grabber
  projects (`%sGrabber\Projects\`), a limits monitor
  (`CDownloadLimits`/`CLimitMonitor`) with exceeded-queue dialogs,
  dial-up era support (RAS enumeration, dial processor classes).
- **Speed limiter**: `CSpeedLimiter` + `SortSpeed()`.
- **Assembly/durability**: fragment-list assembly with explicit UX
  for AV interference ("Access denied while assembling downloaded
  parts… reboot… press resume to assemble"), `FlushFileBuffers`,
  `SetThreadPriority` touches.
- **Browser proxy parity**: troubleshooting text for VPN/proxy
  browser extensions — the engine tries to detect the browser's
  proxy/SOCKS and warns when a VPN-extension-only proxy makes direct
  engine requests fail.
- Quirks worth noting: the engine ships a ~150 KB English-HTML
  **zlib preset dictionary** (used to pre-train compression of its
  own data); a self-update flow via the registration menu; an
  antivirus-exclusion advisory in its own dialogs.

## 4b. Function-level deep-dive: scheduler + connection scaling (D7/D8, session 1)

The follow-up commissioned by the owner (D7:B, started immediately per
D8) decompiled the scheduler and connection-scaling machinery of
`IDMan.exe` at the function level. Because the sandbox cannot
auto-analyze a 2.97 MB code image within its foreground budget, the
work used a bounded no-analysis import plus targeted scripts: anchor
strings located from the §4 inventory, backward function-start
discovery validated by disassembly, MSVC RTTI pointer-chain walks
(type descriptor → complete-object locator → vtable → virtual-method
entry points), and bounded decompilation of the anchor functions, the
`CDownloadQueue`/`CDownload`/`CQueuesArray`/`CSchedulerInfo`/
`CLimitMonitor`/`CDownloadLimits` vtable slots, and one level of
callees — 184 functions in total, selected by evidence, never wholesale.

**Connection scaling (the segment-count policy).** A single decider
routine computes the target connection/segment count for a live
download from a **data-volume signal**: the amount received since the
last reference point, divided by a ~10 KiB unit, clamped between a
per-download minimum and maximum, further clamped by a hard ceiling of
32, and — when the download is marked as a low-class link — by a
ceiling of 4 (narrow class) or 8. The same routine stores the computed
target where the spawn/retire orchestrator reads it. The orchestrator
then walks the per-connection worker array: it retires workers in a
terminal state or carrying one of a specific set of error classes,
computes the desired count as the larger of the current count and the
target, activates idle worker slots up to a floor-respecting bound,
and returns the number spawned. When the decider's target is minimal,
the orchestrator takes the "optional connections not needed" path and
retires only dead workers. This is a **throughput-proportional**
controller — more received data per window ⇒ more connections — which
is a richer signal than our adopted B2 wave-1 controller (3-tick
spawn/retire over an EMA): it uses cumulative volume, not instantaneous
rate, as the spawn signal. The per-download min/max fields double as
the link-class preset mechanism (the wave-2 B2-presets spec can adopt
this shape: per-task floor/ceiling pair plus a link-class ceiling of
4/8 and the global 32 cap).

**Queue persistence and scheduler state.** Each named queue persists
as two small files under a Scheduler directory: a queue-state file
holding the queue's task identifiers as a plain space-separated list
of integers (loaded with a simple scan/parse loop, each parsed value
appended to the in-memory queue), and a per-queue scheduler-settings
file. The main queue (identifier 1) auto-creates default settings when
its file is absent; queue identifier 2 carries a distinct enabled-flag
default. Queue membership itself is a singly linked list of small
nodes (three machine words of payload each) with tail insertion.

**Download limits (the rolling-window quota).** The limit monitor's
configuration lives in the registry under a Scheduler key with four
values: an enable flag, a megabyte count (default 200), a hours window
(default 5), and a show-warning flag (default on); the byte budget is
megabytes × 1 MiB. Functionally: a **megabytes-per-hours rolling
window** that gates further queue processing and warns when exceeded —
plus honest input validation (hours must be 1–23).

**Queue completion actions.** After queue processing finishes, the
scheduler can perform system actions: hang up a modem (dial-up era),
put the machine to sleep or hibernate, or exit the application — with
an explicit wake-timer advisory for scheduled restarts (a Windows
power-settings hint) and a cancel-grace dialog when other downloads are
still in progress.

**Download completion path.** The per-download completion handler
branches on content type: if the content description contains
"video"/"audio", or the target filename's extension is in a
media-extension list (about two dozen entries), completion hands off
to a dedicated media post-processing thread; otherwise it posts a
completion event carrying a small record (time range, byte range,
flags) to the application window. Each download runs as its own worker
thread started below normal priority.

**Session-2 targets (the multi-session effort continues):** the
worker-thread setup that actually splits a file into byte ranges per
connection (the range arithmetic), the queue-state writer (the
save-side of the queue files), and the site-login/refresh-link
routines. The bounded toolchain and scripts for this deep-dive are
backed up with the sandbox toolbox (see RUNBOOK's Ghidra section).

## 4c. Session-2 deep-dive findings (2026-11-24): worker range requests,
response invariants, login/refresh machinery

All functional descriptions, our own words; anchors selected from string
use-sites, functions located by bounded backward-start discovery and
validated decompilation (152 functions this session).

**Worker range-request construction.** The per-connection request builder
emits a ladder of byte-range shapes: a bounded range for a normal segment
(`bytes=<start>-<end>`), an open-ended range for a resumed tail
(`bytes=<start>-`), a bounded probe (`bytes=0-<n>`) and a full open probe
(`bytes=0-`) for size discovery. Functionally this matches the
lease/range-pool model VelocityDM already runs (parity confirmation, not
an adoption).

**Coding discipline on ranged requests.** The builder sends
`Accept-Encoding: identity` for range requests (byte-offset arithmetic
stays valid — a coded body would silently shift every offset), while the
non-ranged default negotiates compression (`gzip, deflate`). A per-task
configured encoding overrides where set. Our engine sends no
Accept-Encoding at all today — the gap behind backlog **B17**.

**Response-side invariant validation.** A dedicated validator inspects
every ranged response head before bytes are trusted: it detects
`multipart/byteranges`-family content types (with boundary harvesting) for
a single-range request, `chunked` transfer encoding, a compressed body
(the gzip/x-gzip/deflate/x-deflate/brotli coding family), and parses
`Content-Length`, `ETag`, and `Content-Range: bytes <s>-<e>/<t>` with
64-bit arithmetic. Second backlog **B17** half.

**Stale-resource detection on resume.** The resume path appends an
`If-Range:` validator header (a tiny dedicated appender function fed from
the download's stored validator; the response validator parses `ETag:`),
so a compliant server answers 200 (resource changed) instead of a 206
that would graft different bytes. Backlog **B18**.

**Expired-address handling (site-login/refresh-link).** The reference
walks an owner-page refresh flow: it downloads the owner web page to
re-derive the address, and a page analyzer parses `http-equiv=refresh`
meta redirects, harvests `http-equiv=Set-Cookie` pseudo-headers, and
GATES the scrape on robots meta `none/nofollow` compliance. A
manual-login flow generates a per-download login page the user opens, and
per-project session/auth state (authorization on/off, login/password,
manual-login URL, logout URL, a logout-deny flag) persists per project
with a schema-version field and a fingerprint value in per-project
storage files. Backlog **B19** adopts the honest minimum (failure
classification + guidance); the full re-scrape machinery is a recorded
non-goal with the robots-compliance constraint carried forward.

**Site-specific CDN adapters (observed, NOT recommended).** The binary
carries compiled-in adapters for at least one signed-URL media CDN: a host
gate, range negotiation via URL QUERY parameters instead of the Range
header (`&bytes=<start>-<end>` shapes), signature-parameter normalization
(`?sig=`/`?signature=` rewriting), and duration/init/length query
parameter handling. We keep our data-driven observe-first posture — no
public specification governs these quirks, and hard-coding third-party
site behavior is a maintenance and compliance liability (same philosophy
as the B15 note).

**Query-range variant (observed, NOT recommended).** The same query-
parameter range convention appears in a generic builder (`bytes=0-`,
`bytes=0-<n>`, `bytes=<start>-<end>` in the URL). Recorded for parity
awareness only.

**REA evidence layer (session 2).** REA 6.10.1 ran the structured PE
inspection (imports/resources/hardening) on the pinned installer and the
extracted engine module, its JavaScript application analyzer produced a
structured inventory of the Firefox XPI (3 parsed scripts; the wave-1
manual §3 read cross-checked), and its Ghidra provider produced the
installer-stub overview. Evidence JSON stayed in the throwaway dir.
New-tool provenance: npm-distributed MIT-licensed client tooling, no
network calls to the studied vendor beyond the pinned artifact URL.

## 4d. Session-16 independent re-derivation (2026-11-24): transport/TLS layer,
request hygiene, FTP state machine, legacy protocols

An independent session rebuilt the toolchain from the RUNBOOK recipes and
re-derived the study end-to-end through the **rea 6.10.1 attach-mode
pipeline** (Ghidra 12.2 provider; caller-prepared bounded-resource project;
`search`/`xrefs`/`decompile` queries — recipe now in `RUNBOOK.md`). The
fail-closed fingerprint gate re-verified the pinned installer byte-exact;
payload extraction independently recovered the engine image via a generic
zlib/zip stream-carve (`scripts/extract_idm_payload.py`, kept as a
cross-check of `unpack_idm.py`). Findings below are our own words; the
first bullet set **confirms prior sections**, the rest adds new
observations.

**Parity confirmations (already covered above):** the four-form worker
range ladder with inclusive-end arithmetic (§4c); `If-Range:`/`ETag:`/
`Last-Modified:` resume validators and the case-insensitive
conditional-header validator stage covering `If-Range`/`If-Match`/
`If-Unmodified-Since` (§4c/B18); chunked transfer decoding and gzip/
deflate content decoding; proxy CONNECT tunneling with per-connection
retry counters (§4b); one native-messaging host name registered under
Chrome/Edge/Firefox NativeMessagingHosts (§3); installer imports and
runtime `LoadLibrary`-ed network stack (§1/§2).

**TLS is the OS Schannel provider, with explicit TLS 1.3 client
enablement.** The engine logs Schannel credential creation, loads
`schannel.dll`, and writes client protocol policy under the system
SCHANNEL registry key including a TLS 1.3 client subkey. WININET is
confined to a utility role — URL cracking/canonicalization, cookie
get/set (browser-session context), and cache-entry lookup — never the
transport. (Ours: rustls with a webpki root store; the observable lesson
is that protocol-version policy is a first-class, externally configurable
setting.)

**Outbound header hygiene is a charset whitelist.** Header values
(user-agent, cookies, referer chain) pass a strict token/field character
allowlist; disallowed characters are silently replaced with `_` rather
than dropped or escaped — injection defense at the value level, in
addition to our existing CRLF/NUL sanitization at the emission boundary.

**Range capability probing has a user-visible no-resume state.**
`Accept-Ranges` is read and `Content-Range` parsed; a server that rejects
ranged requests falls back to single-stream, and a server with no resume
support after a transfer interruption produces an explicit user-facing
state ("cannot resume this file") instead of a silent restart. `Retry-After`
from the server is honored on retryable failures.

**Connections default to `Connection: Keep-Alive` and are reused** across
segment requests of the same download.

**The generic request builder is a slot machine, not a hardcoded template.**
One format family carries ~17 optional `%s` header slots filled from
per-connection state (UA, Accept, Referer, Cookie, Keep-Alive, auth,
conditional validators, proxy credentials…), and there are **two builder
families with different header ordering** (one Host-first, one UA-first) —
per-protocol/per-profile request shapes.

**FTP is a PASV capability state machine.** The FTP layer is a class
hierarchy (`FTPConnection::try_set_pasv_cpbl`, `SendPasv`,
`ProcessPasvReply`, `ProcessPasvOK`, session-level passive-mode setting)
with reply parsing, data-socket IP extraction, and a graceful "server
does not support PASV" fallback; each lifecycle step carries its own
classified error text. (Ours: not implemented; recorded as parity
knowledge for a future FTP stage, spec-first.)

**Legacy protocol family (recorded, non-goal).** Compiled-in RTMP variants
(`rtmp://`, `rtmpt://`, `rtmpe://` with a secure-catch path) and Flash
FCS handshakes (`application/x-fcs` POSTs with a Shockwave Flash
user-agent). Dead formats; same posture as the site-adapter note — no
adoption.

**Process/env behaviors (§2 additions):** configuration hot-reload via a
registry change notification; privileged helper launch via token
inspection/duplication and `CreateProcessAsUserW` (integration helpers in
the right user/session context); the NativeMessagingHosts registration
also covers the CocCoc browser (extends §3's browser list).

## 5. Improvement backlog (mapped to VelocityDM areas)

Legend — target: our code area; tier: provenance (all T2, 2026-11-21,
Ghidra 12.2 / binutils / direct JS read); gate: every item still needs
the standing five-role gate + spec-first commit + `bench-perf.sh` A/B
win with no battery regressions. Priority is a recommendation for
roadmap ordering, not a decision.

**Recommended (clear wins, low risk):** — adoption status tracked here
as S11-i waves close (wave 1: B1/B2/B9 adopted 2026-11-21 under gate
0006, specs in `docs/SPECS/S11-ADOPTIONS.md`; wave 2 = the rest).

| ID | Adopt (as behavior spec) | Target area | Expected gain | Notes |
| --- | --- | --- | --- | --- |
| B1 | Cross-segment data verification: hash-compare overlapping range data across connections; on mismatch auto-fallback to single connection for the file (with a UI reason string) | `vdm-core/src/engine/task.rs` + `http.rs` | stability on inconsistent proxies/CDN edges | **ADOPTED (wave 1, gate 0006)**: content audit of written 256 KiB blocks over fresh connections + bounded single-connection recovery |
| B2 | Optional connections: spawn/retire extra segment connections mid-download based on throughput; segment-count presets per link class (high/medium/low) | `task.rs` + `scheduler.rs` | throughput on fat pipes, resource use on thin ones | **ADOPTED (wave 1, gate 0006)**: dynamic spawn/retire core; link-class presets deferred to wave 2 — **deep-dive §4b maps the reference's preset shape: per-download floor/ceiling pair + link-class ceiling (4/8) + global 32 cap, with a volume-proportional target signal**; **presets ADOPTED (wave 2 engine side, gate 0007)**: `connPreset` knob (auto/high/medium/low) = a base-count ceiling (8/4/2), never a raise; spawn/retire machinery untouched |
| B3 | EXT-X-BYTERANGE parsing + fragment list assembly for HLS | `vdm-core/src/engine/hls.rs` | correctness/range coverage (S7 adjacent) | **ADOPTED (wave 2 engine side, gate 0007)**: `Segment.byterange`, implicit-offset chains per URI, whole-resource reset, 206+short-body fail, AES composition, exact-total accounting, EXT-X-MAP loud-fail (S7 scope preserved) |
| B4 | NM host resilience: persistent port + backoff reconnect (reference uses 1500 ms), host-absence error matching for graceful degradation | `extensions/shared/messaging.js` + `crates/velocitydm-nmh` | stability of browser↔core link | **ADOPTED (wave 2 extension side, gate 0007)**: `shared/nmhost.js` — persistent port preferred with one-shot fallback; backoff 500 ms doubling to 8 s (ours, not the reference's 1500 ms), 30 s-stability reset that also closes the 10-minute reconnect episode; queued commands cap 32 drop-oldest, first-send failure re-queues exactly once; host-absence patterns fail fast so the browser download resumes (the decline path stays correct) |
| B5 | Cookie-jar forwarding: `cookies.getAll({url})` serialization incl. Firefox `firstPartyDomain` on takeover, so authenticated downloads don't fail on cookie split | `extensions/shared/*` | correctness on auth'd sites | **ADOPTED (wave 2 extension side, gate 0007)**: `shared/cookies.js` — the jar rides EVERY add path (context menu, popup, URL-list batches capped at the first 32 per-URL, takeover unchanged); session AND persistent cookies (our choice); NMH `addBatch` gains per-URL `cookiesBy`; cookie values never reach any log channel (explicit test). Firefox CHIPS jars stay an honest documented limitation; pattern batches ride the engine's no-options addBatch surface (gap recorded) |
| B6 | Takeover UX: shelf/UI suppression + cancel-then-resume handoff (never leave a dead browser download when the engine declines); a `captured.html#url`-style in-tab confirmation with auto-back | `extensions/chromium/*`, `extensions/shared/*` | UX clarity + fewer double-downloads | **ADOPTED (wave 2 extension side, gate 0007)**: `shared/takeover.js` + `taken.html` — shelf suppressed while takeover is enabled (feature-detected; Firefox no-ops), filename participation suggests our name once on our takeover (never fights a user rename), decline → browser resume + the static note surfaces in the popup's error area, the static self-contained confirmation page reads the hash once via textContent only and history.back()s immediately (rides content-script video takeovers only); the shelf + page behaviors owe the owner's D5:A runtime verification |
| B7 | Video detection hardening in content scripts: geometry gating (min width/height, `videoWidth`/`networkState` while `readyState<1`), transparent-overlay visibility filter, MutationObserver+ResizeObserver on players, inline-script URL scraping, site-specific selector packs | `extensions/shared/detector.js` | fewer false positives (resource + UX) | **ADOPTED (wave 2 extension side, gate 0007)**: geometry gate 200×120 ours (NOT the reference's 15×10) + 640×360 prominence tier, state gate, bounded 32-step ancestor visibility walk, inline scan capped at the first 128 KiB with a letter-boundary regex, one MutationObserver (media-tag-filtered, 250 ms debounce) + a ResizeObserver on candidates active only during bounded windows with full teardown; site packs data-driven and OFF by default; honest overhead witness: a synthetic 5000-candidate gated pass measured 4.63 ms in the dev sandbox |
| B8 | MIME→extension normalization table for filename choices | `extensions/shared/*` + core naming | correctness | ~100-entry data table, ours from scratch; **core side ADOPTED (wave 2 engine side, gate 0007)**: `vdm-core/src/mime.rs` (110 entries from the public IANA registry + widely-served vendor aliases; parameters/case normalized; octet-stream deliberately absent) applied only when the URL yields no extension — never overrides an explicit filename or an existing extension; **extension side ADOPTED (wave 2 extension side)**: the generated copy is now a UMD module (the ESM emit could not load in a classic service worker — caught at consumer wiring), the sniffed-video list gains a filename hint, and the onDeterminingFilename suggestion + takeover filename mirror use the table; drift tripwire enforces Rust/JS equality |
| B9 | Assembly-conflict UX: explicit "another app (AV) holds the file; reboot+resume to assemble" messaging and retry path | `vdm-core/src/engine/task.rs` + frontend surfaces | recoverability | **ADOPTED (wave 1, gate 0006)**: lock-classification + bounded finalize retry + resumable failure |
| B17 | Ranged-request coding discipline (`Accept-Encoding: identity` on range requests) + ranged-response invariants (reject compressed / multipart / Content-Range-mismatched responses before any byte is trusted) | `vdm-core/src/engine/task.rs` (+ win7) | eliminates the silent-corruption class on misbehaving servers | **ADOPTED (wave 3, gate 0009)** — public-spec source RFC 9110 §8.4/§12.5.3/§14; specs `docs/SPECS/S11-ADOPTIONS-WAVE3.md` |
| B18 | If-Range validator on in-run ranged requests; 200-at-offset>0 classified definitively as "resource changed" (fast honest failure instead of retry churn) | `vdm-core/src/engine/task.rs` (+ win7) | stability + honest failure reasons on changed resources | **ADOPTED (wave 3, gate 0009)** — RFC 9110 §13.1.5; cross-restart validator persistence is a recorded non-goal (ResumeState untouched) |
| B19 | Expired-link guidance: 403/410 classified as refreshable with actionable guidance in the extension popup | `vdm-core/src/engine/task.rs` + `extensions/chromium/popup.js` | UX recoverability | **ADOPTED (wave 3, gate 0009)** — honest minimum; the reference's full owner-page re-scrape (meta-refresh, Set-Cookie http-equiv, robots meta gating) is a recorded non-goal for any future re-scrape feature to honor |
| B20 | Outbound header hygiene: value-level charset discipline layered on the emission boundary | `vdm-core/src/http.rs` (+ win7) | robustness: a malformed caller value degrades instead of aborting | **ADOPTED (wave 4, gate 0011)** — re-spec from RFC 9110 §5.5/§5.6.2; the panel narrowed the §4d "full whitelist": tchar name refusal (tightening), CR/LF/NUL value refusal unchanged, other controls + DEL replace-with-`_` surfaced by notice, obs-text passes (UTF-8 cookies keep working); specs `docs/SPECS/S11-ADOPTIONS-WAVE4.md` |
| B21 | Resume honesty: explicit user-visible no-resume state instead of silent restart; Retry-After honored on retryable failures | `vdm-core/src/engine/task.rs` (+ win7) | honest failure reasons + data-loss-class fix (stale `.vdmctl` vs truncated `.part` coherency) | **ADOPTED (wave 4, gate 0011)** — RFC 9110 §10.2.3 delta-seconds only, 429/503, 60 s cap, stop-responsive; the panel upgraded the item with the `.vdmctl`/`.part` coherency rule (silent zero-holes risk found in our own engine) |
| B22 | TLS protocol-version policy as a first-class externally configurable setting | `vdm-core/src/tls.rs` + settings (+ win7) | parity: min-version floor configurable (rustls floor = TLS 1.2, so the knob is floor-raising only) | **ADOPTED (wave 4, gate 0011)** — additive `tlsMin` settings knob (`default|tls12|tls13`, default = zero change), fail-closed at wrap; wire-level claim docs-only (no TLS test server in-sandbox) |
| B23 | Keep-alive connection reuse across segment requests | `vdm-core/src/http.rs` + `engine/task.rs` | throughput (handshake amortization) | **ALREADY OURS — witnessed (wave 4, gate 0011)**: `Connection: keep-alive` default + worker conn recycling predate the study; `keepalive_reuse_across_leases` pins the count; recorded as parity confirmation, not adoption |
| B24 | CocCoc browser NativeMessagingHosts registration in packaging | `packaging/nmh/install-nmh.sh` + `packaging/linux/install.sh` | browser-integration parity (regional market) | **DEFERRED (wave 4, gate 0011)** — the exact CocCoc per-OS config dir names could not be empirically verified in the sandbox (AUR bot-blocked, official deb endpoint 404); an unverified path in an existence-gated install loop is a silent no-op. Pre-condition: observe a real CocCoc install's NMH paths (Linux user dir + Windows registry hive); fix the linux uninstall-list asymmetry (vivaldi/brave missing) in the same commit |

**Candidate (worth a spec + bench, decide at the gate):**

| ID | Adopt | Target area | Notes |
| --- | --- | --- | --- |
| B10 | DNR session rules (regex + main_frame) as the conflict-free handoff mechanism alongside existing webRequest flow; `redirectUrl:"javascript:void(0)"` suppression trick | `extensions/chromium/background.js` | only if MV3 tightening forces it; we already avoid `webRequestBlocking` |
| B11 | MAIN-world body re-association: tag page fetch/XHR with marker params and re-read bodies at the webRequest layer for video-on-XHR sites | `extensions/shared/*` + detector | high complexity; scope strictly to sites where content-script observation is insufficient |
| B12 | Session-scoped IPC naming (pipe/event names suffixed with the OS session id) + peer-identity verification on the Windows daemon path | `crates/velocitydm/src/daemon.rs` (Windows IPC) | our loopback TCP + trust rules already enforce the boundary; session naming is defense-in-depth for any future named-pipe transport. Peer verification applies if we add pipe transports; `cmd.exe`-rejection pattern is the reference's, ours would be a from-scratch spec |
| B13 | Per-queue persisted scheduler state (small per-queue files rather than one monolith) | `scheduler.rs` + state.rs | cheap, aids crash recovery; weigh against our existing state serialization | **deep-dive §4b maps the reference model: per-queue id-list file + per-queue scheduler-settings file, main queue defaults auto-created**; **ADOPTED (wave 2 engine side, gate 0007)**: single-queue rules persist to `<download-dir>/.vdm/scheduler.vdmq` (schema v1, ours — atomic temp+rename writes, tolerant loads; saved on every rule change + clean shutdown, loaded at engine start); named multi-queues stay parked |
| B14 | TS→MP4 remux for HLS TS downloads (codec-aware) | `engine/hls.rs` (S7 family) | real feature work; gate separately — likely S7+ |
| B15 | Anvato-style `#EXT-…-SEGMENT-INFO:`/site-specific segment-info parsing framework | `hls.rs`/dash (S6/S7) | data-driven site packs; observe first |
| B16 | Browser-proxy propagation: extension reports the browser's effective proxy to the host; warn on VPN-extension mismatches | `extensions/shared/*` + `vdm-core/src/proxy.rs` | parity win for auth'd/proxied sites | **core side ADOPTED (wave 2 engine side, gate 0007)**: HttpOptions.browserProxy (additive) + probe-guarded adoption (suggest, never override), differs/unreachable warnings on the add reply + snapshot notice, NMH shape validation; **extension side ADOPTED (wave 2 extension side)**: `shared/proxyreport.js` resolves chrome.proxy.settings (system/mode-only reports carry no endpoint; fixed endpoints limited to http/https/socks5), the additive `browserProxy` field rides every add path with a 60 s cache, and the manifest gains its one `proxy` permission (the wave's only permission delta) |

**Observed, NOT recommended for adoption (documented deliberately):**

| ID | Method | Why not |
| --- | --- | --- |
| N1 | Kernel WFP/TDI interception layer with on-the-fly body decompression | kernel signing, AV friction, platform fragility, out of scope for a GPL user-space product; our extension+NMH layer covers the same user value honestly |
| N2 | Runtime `LoadLibrary` of the network stack (no static imports) | Rust static linking gives us the same benefit trivially; the trick exists to dodge import-table scanning and legacy-OS survival |
| N3 | Custom zlib container + manifest-driven installer | our Inno installer is better overall; complexity not worth ~1 MB |
| N4 | Direct XPI write into Firefox profile directories at install time | legacy-Firefox-only pattern; modern FF WebExtension + NMH JSON is the supported path we already ship |
| N5 | COM `CIDMLinkTransmitter` link intake + typelib registration machinery | Windows-only legacy convenience; our JSON-RPC over loopback is already cross-platform and trust-bounded |
| N6 | Dial-up/RAS era scheduling (dial processor classes, hangup) | dead feature class for our user base |
| N7 | Configuration hot-reload via registry change notification + privileged helper launch via token inspection/duplication (`CreateProcessAsUserW`) | Windows-registry-era mechanics; N/A for a GPL cross-platform user-space product — our config loads from config.json at daemon start and the NMH is launched by the browser itself (§2 additions, gate 0011 recorded) |
| N8 | Format-string request builder (~17 optional `%s` header slots, two builder families with different header ordering) | the slot machine is the reference's C-era shape; our typed `Request`/`ClientCfg` builder (http.rs) is the deliberate Rust shape — same capability (per-connection header state), no adoption (gate 0011 recorded) |

The `idmindex.dll` SQLite embed is noted as a design reference for any
future local index/search need (our state model does not need it today).
The FTP PASV capability state machine (§4d) stays the recorded parity
knowledge for a future FTP stage, spec-first at that stage's own gate —
pre-condition recorded at gate 0011: any FTP adoption must constrain the
server-supplied PASV data target (an SSRF/bounce/port-scan primitive):
connect only to the control-connection host by default and refuse
loopback/link-local targets.

## 6. Scope, limitations, and what this is not

- No dynamic analysis (T3) was performed and no studied binary was
  executed: the sandbox has no Windows, and T3 needs an
  owner-provisioned Windows VM per the S11 brief. T1 black-box
  measurement is likewise pending that VM.
- `IDMan.exe`'s 2.97 MB of code was analyzed at the strings/imports/
  RTTI level in the original study; the commissioned function-level
  deep-dive (D7:B / D8:now) has covered the scheduler,
  queue-persistence, limits, and connection-scaling machinery (184
  functions, §4b) and, in session 2 (2026-11-24), the worker range
  arithmetic, response invariants, and the site-login/refresh-link
  machinery (152 further functions incl. all session-2 anchors, §4c) —
  still bounded, never wholesale.
- Findings are for build 6.43.18 only; the vendor rotates builds and
  the fail-closed re-pin policy stands.
- This document contains no proprietary code, structures copied for
  implementation reasons, resources, string tables, or UI — only
  functional descriptions. Every adopted technique is a re-spec from
  this document's behavior descriptions, spec-first, under the
  incorporation gate.
