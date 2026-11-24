//! HTTP/HTTPS/HLS download task: probing, worker pool with work-stealing
//! leases, mirror failover, AIMD block sizing, token-bucket pacing,
//! crash-safe `.part`/`.vdmctl` state, checksum verification.

use crate::engine::events::{EngineShared, TaskState};
use crate::engine::state::ResumeState;
use crate::error::{Error, Result};
use crate::http::{self, ClientCfg, Conn, Request, Url};
use crate::range_pool::RangePool;
use crate::token_bucket::TokenBucket;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Per-download options (HTTP and HLS). Serialized camelCase for the RPC API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HttpOptions {
    #[serde(default)]
    pub connections: usize,
    #[serde(default)]
    pub mirrors: Vec<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub out_dir: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub rate_limit_bps: u64,
    #[serde(default)]
    pub checksum_algo: Option<String>,
    #[serde(default)]
    pub checksum_hex: Option<String>,
    #[serde(default)]
    pub priority: u8,
    #[serde(default)]
    pub start_at: Option<u64>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub proxy: Option<crate::proxy::ProxyConfig>,
    /// HLS variant: "best" (default), "worst", or a bandwidth number.
    #[serde(default)]
    pub quality: String,
    /// Link-class connection preset (S11-i B2p): `auto` (default — today's
    /// behavior), `high` (base ceiling 8), `medium` (4), `low` (2). A preset
    /// is a ceiling on the base connection count, never a raise; the B2
    /// spawn/retire machinery is untouched. Not persisted in `ResumeState`
    /// (an options-time knob).
    #[serde(default = "default_conn_preset")]
    pub conn_preset: String,
    /// The browser-reported effective proxy (S11-i B16): the engine may
    /// adopt it when the task has no explicit proxy (adoption is guarded by
    /// a one-connect reachability probe); an explicit task proxy always
    /// wins (a differing browser proxy warns). Additive, camelCase
    /// `browserProxy`; never persisted in `ResumeState` (options-time knob).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_proxy: Option<crate::proxy::ProxyConfig>,
    /// Do not resume: discard any `.part` state.
    #[serde(default)]
    pub fresh: bool,
    /// Disable the cross-connection content audit (S11-i B1) for this
    /// task. Default `false` (audits on).
    #[serde(default)]
    pub no_audit: bool,
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub final_url: Url,
    pub size: u64,
    pub accepts_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub filename: String,
    pub content_type: Option<String>,
}

// ---- S11-i B2p: link-class connection presets ----

/// Base-connection ceiling per preset class (ours).
const PRESET_HIGH: usize = 8;
const PRESET_MEDIUM: usize = 4;
const PRESET_LOW: usize = 2;

fn default_conn_preset() -> String {
    "auto".into()
}

/// Validate a connection preset at an add boundary (engine `add_download`
/// and the NMH host both call this). Accepted: `""` (the serde-default
/// shape), `auto`, `high`, `medium`, `low` — case-insensitive, trimmed.
/// Anything else is rejected with our message.
pub fn validate_conn_preset(raw: &str) -> crate::Result<String> {
    let n = raw.trim().to_ascii_lowercase();
    match n.as_str() {
        "" | "auto" | "high" | "medium" | "low" => Ok(n),
        other => Err(crate::Error::Http(format!(
            "unknown connection preset '{}'",
            other
        ))),
    }
}

fn preset_ceiling(valid: &str) -> Option<usize> {
    match valid.trim() {
        "high" => Some(PRESET_HIGH),
        "medium" => Some(PRESET_MEDIUM),
        "low" => Some(PRESET_LOW),
        _ => None, // auto / ""
    }
}

/// Effective base connection count after the preset ceiling (S11-i B2p):
/// `min(preset ceiling, size-scaled base)` — a ceiling, never a raise.
fn apply_preset_ceiling(base: usize, preset: &str) -> usize {
    match preset_ceiling(preset) {
        Some(c) => base.min(c),
        None => base,
    }
}

// ---- S11-i B16: browser-proxy propagation (engine side) ----

/// A task proxy that differs from the browser's proxy warns (ours, static).
pub(crate) const WARN_PROXY_DIFFERS: &str =
    "task proxy differs from the browser's proxy; downloads may not follow the browser's route";
/// A reported (browser) proxy that failed the reachability probe is dropped
/// with this warning (ours, static).
pub(crate) const WARN_PROXY_UNREACHABLE: &str =
    "the browser's proxy is not reachable from the engine; a VPN-extension proxy is likely the cause";

/// Resolve the effective task proxy (S11-i B16 — suggest, never override):
/// an explicit task proxy always wins (a differing browser proxy yields
/// the differs-warning); otherwise the browser-reported proxy is adopted
/// only when a one-connect reachability probe reaches it — unreachable
/// means dropped, with the warning. Pure decision core (unit-testable).
pub(crate) fn resolve_task_proxy(
    explicit: Option<&crate::proxy::ProxyConfig>,
    browser: Option<&crate::proxy::ProxyConfig>,
) -> (Option<crate::proxy::ProxyConfig>, Option<&'static str>) {
    match (explicit, browser) {
        (Some(task), Some(browser)) => (
            Some(task.clone()),
            (task != browser).then_some(WARN_PROXY_DIFFERS),
        ),
        (Some(task), None) => (Some(task.clone()), None),
        (None, Some(browser)) => {
            if browser.probe_reachable(Duration::from_secs(2)) {
                (Some(browser.clone()), None)
            } else {
                (None, Some(WARN_PROXY_UNREACHABLE))
            }
        }
        (None, None) => (None, None),
    }
}

/// HEAD (fallback ranged GET) probe with redirect resolution.
pub fn probe(url: &Url, cfg: &ClientCfg) -> Result<ProbeResult> {
    probe_with_meta(url, cfg).map_err(|pf| Error::Http(pf.msg))
}

/// Structured probe failure (S11-i B21b): carries the HTTP status and the
/// parsed Retry-After so the retry ladders decide structurally, never by
/// error-string matching.
pub(crate) struct ProbeFailure {
    pub msg: String,
    pub status: Option<u16>,
    pub retry_after: Option<Duration>,
}

impl ProbeFailure {
    /// Transient-status classification first (B21b: 429/503/5xx are
    /// structurally retryable); transport failures fall back to
    /// `is_retryable` (connect/read strings — unchanged semantics).
    pub(crate) fn retryable(&self) -> bool {
        match self.status {
            Some(s) => is_transient_status(s),
            None => is_retryable(&Error::Http(self.msg.clone())),
        }
    }

    /// The bounded server-controlled wait, when the status carries one.
    pub(crate) fn throttle_wait(&self) -> Option<Duration> {
        match self.status {
            Some(s) => retry_after_delay(s, self.retry_after),
            None => None,
        }
    }
}

pub(crate) fn probe_with_meta(
    url: &Url,
    cfg: &ClientCfg,
) -> std::result::Result<ProbeResult, ProbeFailure> {
    let (final_url, mut resp) = match http::resolve_redirects(url, cfg, 5, "HEAD") {
        Ok(v) => v,
        Err(_) => match http::resolve_redirects(url, cfg, 5, "GET") {
            Ok(v) => v,
            Err(e) => {
                return Err(ProbeFailure {
                    msg: e.to_string(),
                    status: None,
                    retry_after: None,
                })
            }
        },
    };
    let head_status = resp.status;
    let head_ok = head_status == 200;
    let mut size = resp.content_length().unwrap_or(0);
    let mut accepts_ranges = resp
        .header("accept-ranges")
        .map(|v| v.to_ascii_lowercase().contains("bytes"))
        .unwrap_or(false);
    let mut etag = resp.header("etag").map(|s| s.to_string());
    let last_modified = resp.header("last-modified").map(|s| s.to_string());
    let content_type = resp.header("content-type").map(|s| s.to_string());
    let disposition = resp.header("content-disposition").map(|s| s.to_string());
    let _ = resp.read_body(1024);
    let mut fatal: Option<ProbeFailure> = if head_status >= 400 {
        Some(ProbeFailure {
            msg: status_failure_reason(head_status),
            status: Some(head_status),
            retry_after: parse_retry_after(resp.header("retry-after")),
        })
    } else {
        None
    };
    if !head_ok || size == 0 || !accepts_ranges {
        match ranged_get(&final_url, cfg, 0, 0) {
            Ok(mut r2) => {
                if r2.status >= 400 {
                    if fatal.is_none() {
                        fatal = Some(ProbeFailure {
                            msg: status_failure_reason(r2.status),
                            status: Some(r2.status),
                            retry_after: parse_retry_after(r2.header("retry-after")),
                        });
                    }
                } else {
                    fatal = None;
                    // S11-i B17: an invalid ranged response yields no usable
                    // size here — the probe stays advisory (fail-open to the
                    // other size sources, never fatal).
                    let r17 = classify_ranged_response(r2.status, &r2.headers, (0, 0));
                    if r2.status == 206 && r17.is_none() {
                        accepts_ranges = true;
                        if let Some(cr) = r2.header("content-range") {
                            if let Some(total) = cr.rsplit('/').next() {
                                size = total.trim().parse().unwrap_or(size);
                            }
                        }
                    } else if r2.status != 206 && size == 0 {
                        size = r2.content_length().unwrap_or(0);
                    }
                    if etag.is_none() {
                        etag = r2.header("etag").map(|s| s.to_string());
                    }
                }
                let _ = r2.read_body(1024);
            }
            Err(_) => {
                // keep the HEAD-level fatal status, if any
            }
        }
    }
    if let Some(pf) = fatal {
        return Err(pf);
    }
    let filename = disposition
        .and_then(|d| filename_from_disposition(&d))
        .or_else(|| crate::util::filename_from_url(&final_url.to_string_full()))
        .unwrap_or_else(|| "download".to_string());
    Ok(ProbeResult {
        final_url,
        size,
        accepts_ranges,
        etag,
        last_modified,
        filename,
        content_type,
    })
}

/// Whether an error is worth retrying (transient network / 5xx).
pub fn is_retryable(e: &Error) -> bool {
    match e {
        Error::Io(_) => true,
        Error::Http(msg) => {
            msg.contains("HTTP 5") || msg.contains("connect") || msg.contains("read")
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// S11-i B21b (docs/SPECS/S11-ADOPTIONS-WAVE4.md; RFC 9110 §10.2.3,
// delta-seconds form only — no date-parsing dependency, lock parity both
// trees). Status threading is structural: callers pass the response status
// and header, never error strings.
// ---------------------------------------------------------------------------

/// Retry-After delta-seconds parse: exactly `\d{1,6}`. The HTTP-date form
/// and anything invalid yield None (the caller's existing ladder applies).
fn parse_retry_after(v: Option<&str>) -> Option<Duration> {
    let v = v?.trim();
    if v.is_empty() || v.len() > 6 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    v.parse::<u64>().ok().map(Duration::from_secs)
}

/// Server-controlled wait cap: bounds the worst case at 60 s × ≤32 workers.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(60);

/// Statuses where the server explicitly throttles with a wait hint.
fn is_throttled(status: u16) -> bool {
    status == 429 || status == 503
}

/// Structural transient-status classification: 5xx plus the throttle
/// statuses are worth retrying; other 4xx are not. (Closes the pre-wave
/// inconsistency where run_single's formatted error never matched
/// `is_retryable` and 5xx was never retried there.)
fn is_transient_status(status: u16) -> bool {
    is_throttled(status) || status >= 500
}

/// The bounded server-controlled wait: on 429/503 with a parsed delta,
/// `min(delta, 60 s)`; otherwise None (the caller's existing ladder
/// applies, byte-identical to the pre-wave behavior).
fn retry_after_delay(status: u16, ra: Option<Duration>) -> Option<Duration> {
    if !is_throttled(status) {
        return None;
    }
    ra.map(|d| d.min(RETRY_AFTER_CAP))
}

/// Stop-aware sleep for Retry-After waits only: short ticks re-check the
/// stop/pause flags so a 60 s server wait never delays a user stop. The
/// existing short backoff sleeps stay as-is (bounded ≤ 4 s).
fn stop_aware_sleep(control: &crate::engine::events::TaskControl, total: Duration) {
    const TICK: Duration = Duration::from_millis(100);
    let mut slept = Duration::ZERO;
    while slept < total {
        if control.is_stopped() || control.is_paused() {
            return;
        }
        let step = TICK.min(total - slept);
        std::thread::sleep(step);
        slept += step;
    }
}

fn ranged_get(url: &Url, cfg: &ClientCfg, start: u64, end: u64) -> Result<http::Response> {
    let mut req = Request::get(url).header("Range", &format!("bytes={}-{}", start, end));
    // S11-i B17: ranged requests pin the identity coding (RFC 9110 §12.5.3)
    // so byte-offset arithmetic cannot shift under a content coding —
    // unless the caller already chose an encoding (user intent wins).
    if !has_accept_encoding(&cfg.extra_headers) && !has_accept_encoding(&req.headers) {
        req = req.header("Accept-Encoding", "identity");
    }
    let mut conn = Conn::connect(url, cfg)?;
    conn.request(&req, cfg)
}

// ---------------------------------------------------------------------------
// S11-i B17/B18/B19 helpers (docs/SPECS/S11-ADOPTIONS-WAVE3.md; public-spec
// sources: RFC 9110 §8.4 / §12.5.3 / §13.1.5 / §14 — behavior re-spec'd from
// the committed wave-3 spec, not from any binary).
// ---------------------------------------------------------------------------

/// True when the caller already supplies an Accept-Encoding header
/// (case-insensitive): user intent wins, never emit a second value.
fn has_accept_encoding(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("accept-encoding"))
}

/// Parse `Content-Range: bytes <start>-<end>/<total|*>` (RFC 9110 §14.4;
/// the range unit compares case-insensitively).
fn parse_content_range(v: &str) -> Option<(u64, u64)> {
    let rest = v.trim();
    if rest.len() < 5 || !rest[..5].eq_ignore_ascii_case("bytes") {
        return None;
    }
    let rest = rest[5..].trim();
    let (range, _) = rest.split_once('/')?;
    let (s, e) = range.trim().split_once('-')?;
    Some((s.trim().parse().ok()?, e.trim().parse().ok()?))
}

/// Why a response to a ranged request is rejected before any of its bytes is
/// trusted (S11-i B17). Header-only checks, off the hot path.
#[derive(Debug, PartialEq, Eq)]
enum RangedInvalid {
    /// A content coding was announced (byte-offset arithmetic would shift).
    Compressed,
    /// A multipart/byteranges-family reply to a single-range request.
    Multipart,
    /// 206 whose Content-Range does not exactly match the requested range.
    ContentRangeMismatch,
}

impl RangedInvalid {
    fn reason(&self) -> &'static str {
        match self {
            RangedInvalid::Compressed => "compressed ranged response",
            RangedInvalid::Multipart => "multipart ranged response",
            RangedInvalid::ContentRangeMismatch => "content-range mismatch",
        }
    }
}

/// Classify a response to a ranged request for `want: (start, end)`.
/// Severity is the caller's: worker = retry-path invalid; audit =
/// inconclusive (never a B1 conflict signal); probe = no usable size.
fn classify_ranged_response(
    status: u16,
    headers: &[(String, String)],
    want: (u64, u64),
) -> Option<RangedInvalid> {
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    if header("content-encoding").is_some() {
        return Some(RangedInvalid::Compressed);
    }
    if let Some(ct) = header("content-type") {
        let low = ct.to_ascii_lowercase();
        if low.contains("multipart/byteranges") || low.contains("multipart/x-byteranges") {
            return Some(RangedInvalid::Multipart);
        }
    }
    if status == 206 {
        return match header("content-range").and_then(parse_content_range) {
            Some(range) if range == want => None,
            _ => Some(RangedInvalid::ContentRangeMismatch),
        };
    }
    None
}

/// S11-i B18 (RFC 9110 §13.1.5): with a validator sent, a 200 full body for
/// a ranged request at offset > 0 is a DEFINITIVE changed-resource verdict —
/// it cannot heal via retries, so the worker fails fast instead of churning
/// MAX_RETRIES across every mirror.
fn ranged_200_is_changed(status: u16, pos: u64, validator_sent: bool) -> bool {
    status == 200 && pos > 0 && validator_sent
}

/// S11-i B18: the in-memory If-Range validator (etag preferred, else
/// last-modified). Deliberately NOT persisted — `ResumeState` stays
/// untouched; after a restart the task resumes without it (the B17
/// invariants and the B1 audit still guard those bytes).
fn if_range_validator(etag: Option<&str>, last_modified: Option<&str>) -> Option<String> {
    etag.map(|s| s.to_string())
        .or_else(|| last_modified.map(|s| s.to_string()))
}

/// S11-i B18 fast-fail reason (distinct namespace from the B19 marker).
const RESOURCE_CHANGED_MSG: &str = "resource changed since download started";

/// S11-i B19: user-facing status classification. 403/410 carry a stable
/// marker the frontends' note channel acts on; every other status keeps its
/// historical exact string (tests and callers depend on them).
const REFRESHABLE_MARKER: &str = "link may be expired or forbidden";

fn status_failure_reason(status: u16) -> String {
    if status == 403 || status == 410 {
        format!(
            "HTTP {} ({} — re-open the source page and re-grab the link)",
            status, REFRESHABLE_MARKER
        )
    } else {
        format!("HTTP {}", status)
    }
}

fn filename_from_disposition(d: &str) -> Option<String> {
    for part in d.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("filename*=") {
            let decoded = v
                .splitn(3, '\'')
                .nth(2)
                .map(crate::util::percent_decode)
                .unwrap_or_else(|| v.to_string());
            let name = crate::util::sanitize_filename(&decoded);
            if !name.is_empty() {
                return Some(name);
            }
        } else if let Some(v) = part.strip_prefix("filename=") {
            let v = v.trim_matches('"');
            let name = crate::util::sanitize_filename(v);
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Environment the engine passes into a task.
pub struct TaskEnv {
    pub download_dir: PathBuf,
    pub default_connections: usize,
    pub user_agent: String,
    pub proxy: Option<crate::proxy::ProxyConfig>,
    pub global_bucket: Arc<TokenBucket>,
}

enum WorkerExit {
    Done,
    Paused,
    Failed(String),
    /// S11-i B2: optional worker retired gracefully (after its lease).
    Retired,
    /// S11-i B1: torn down by the single-connection recovery reset.
    Reset,
}

// ---------------------------------------------------------------------------
// Paper-informed scheduling helpers.
//
// References (see docs/ALGORITHMS.md "Research notes"):
// - Rodriguez et al., "Parallel-Access for Mirror Sites in the Internet":
//   spread ranges across mirrors and reassign them based on measured
//   per-mirror throughput (kills the slow-mirror straggler).
// - Koo et al., "Analysis of Parallel Downloading for Large File
//   Distribution": chunk count/size trades setup overhead against load
//   imbalance; guided (remaining-aware) allocation beats fixed splits.
// - Guided self-scheduling (Polychronopoulos & Kuck): chunk = remaining /
//   f(workers), clamped — large chunks early, small chunks in the endgame.
// - Padhye et al., "Modeling TCP Throughput" (SIGCOMM'98): per-connection
//   throughput bounded by RTT/loss; AIMD at the application layer (below).
// ---------------------------------------------------------------------------

/// Minimum bytes a connection should carry to amortize its setup cost
/// (TCP + TLS handshake, slow start) — Koo et al.'s overhead model.
const MIN_BYTES_PER_CONN: u64 = 1024 * 1024;
const LEASE_MIN: u64 = 512 * 1024;
const LEASE_MAX: u64 = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// S11-i adoption wave 1 (specs: docs/SPECS/S11-ADOPTIONS.md, gate 0006).
// Provenance: T2 reference-product study (docs/RESEARCH-IDM-STUDY.md) —
// behavior re-specified in our own words, clean-room barrier honored.
// ---------------------------------------------------------------------------

/// B1: audit anchor block size.
const AUDIT_BLOCK: u64 = 256 * 1024;
/// B1: new completed bytes required between audit starts.
const AUDIT_STRIDE: u64 = 8 * 1024 * 1024;
/// B1: full-file recoveries allowed per task (third mismatch fails).
const AUDIT_RECOVERIES_MAX: u32 = 2;
const NOTICE_RECOVERY: &str =
    "cross-connection data mismatch detected; restarting the file over a single connection";

/// S11-i B20: surfaced when a caller-supplied header value carried control
/// characters and was sanitized before emission (never the value content).
const NOTICE_SANITIZED: &str =
    "one or more request header values contained control characters and were sanitized before sending";

/// S11-i B21a: the explicit no-resume state — generic by design (it never
/// embeds the URL, query, or path; signed-URL tokens must not leak into the
/// notice channel).
const NOTICE_NO_RESUME: &str = "server does not support resume — the download restarts from zero; partial progress was discarded";

/// B2: maximum optional (dynamically spawned) workers per task.
const OPT_EXTRA_MAX: usize = 8;
/// B2: minimum file size for optional connections.
const OPT_MIN_SIZE: u64 = 16 * 1024 * 1024;
/// B2: minimum remaining bytes to bother spawning.
const OPT_MIN_REMAINING: u64 = 8 * 1024 * 1024;
/// B2: retire when remaining bytes per live connection fall below this.
const OPT_RETIRE_MIN_PER: u64 = 4 * 1024 * 1024;
/// B2: spawn when aggregate speed is below this fraction of the live
/// connections' demonstrated capacity.
const OPT_SPAWN_HEADROOM: f64 = 0.55;
/// B2: retire when aggregate speed is at/above this fraction of what
/// live-1 connections demonstrated.
const OPT_RETIRE_HEADROOM: f64 = 0.85;
/// B2: consecutive decision ticks required to act (anti-flap).
const OPT_TICKS_TO_ACT: u32 = 3;

// S11-i B9: file-lock conflict messages (ours).
const LOCK_FINALIZE_MSG: &str = "output file is locked by another application (for example antivirus or a media player); the download is complete — resume the task to retry saving it";
const LOCK_WRITE_MSG: &str =
    "file is locked by another application; pause the task and resume when the lock is released";

/// Guided lease size (guided self-scheduling): `remaining / workers`
/// clamped to [512 KiB, 16 MiB]. Early in a transfer leases are large so
/// each request amortizes its RTT over many bytes (Koo et al.: request
/// overhead vs imbalance); near the end the schedule shrinks geometrically
/// to the floor so every worker shares the tail instead of one grinding
/// through a large final chunk.
fn guided_lease(remaining: u64, workers: u64) -> u64 {
    (remaining / workers.max(1)).clamp(LEASE_MIN, LEASE_MAX)
}

/// Cap the connection count by file size so each connection carries at
/// least ~1 MiB; opening 16 connections for a 2 MiB file is setup-dominated
/// (Koo et al.: efficiency ~ S / (S + N*C)).
fn scale_connections(requested: usize, size: u64) -> usize {
    let by_size = (size / MIN_BYTES_PER_CONN).max(1) as usize;
    requested.max(1).min(by_size)
}

/// Shared per-mirror statistics for throughput-weighted selection
/// (Rodriguez et al.). All fields are relaxed atomics: races only cost a
/// stale sample, never correctness.
struct MirrorStat {
    /// EMA of per-lease throughput in bytes/s (0 = unmeasured).
    speed_ema: AtomicU64,
    /// Recent failures; decays to 0 on any success.
    fails: AtomicU32,
    /// Do not pick this mirror before this `now_ms()` timestamp.
    cooldown_until: AtomicU64,
    /// Completed leases (test visibility).
    leases: AtomicU64,
}

impl MirrorStat {
    fn new() -> Self {
        MirrorStat {
            speed_ema: AtomicU64::new(0),
            fails: AtomicU32::new(0),
            cooldown_until: AtomicU64::new(0),
            leases: AtomicU64::new(0),
        }
    }

    fn score(&self, now_ms: u64) -> f64 {
        if now_ms < self.cooldown_until.load(Ordering::Relaxed) {
            return 0.0;
        }
        let ema = self.speed_ema.load(Ordering::Relaxed) as f64;
        let fails = self.fails.load(Ordering::Relaxed).min(6) as i32;
        // geometric penalty: one transient fail barely dents a much-faster
        // mirror; consecutive failures dominate (anti-flap to broken hosts)
        ema / 4.0_f64.powi(fails)
    }

    fn record_success(&self, speed_bps: u64) {
        self.fails.store(0, Ordering::Relaxed);
        self.leases.fetch_add(1, Ordering::Relaxed);
        let prev = self.speed_ema.load(Ordering::Relaxed);
        let next = if prev == 0 {
            speed_bps
        } else {
            (prev * 7 + speed_bps) / 8
        };
        self.speed_ema.store(next, Ordering::Relaxed);
    }

    fn record_fail(&self) {
        let f = self.fails.fetch_add(1, Ordering::Relaxed) + 1;
        let cd = crate::util::now_ms() + 2000 * f.min(4) as u64;
        self.cooldown_until.store(cd, Ordering::Relaxed);
    }
}

/// Pick the best-scoring mirror; the current one wins ties so healthy
/// connections are not needlessly torn down.
fn pick_mirror(stats: &[MirrorStat], current: usize, now_ms: u64) -> usize {
    let mut best = current;
    let mut best_score = stats[current].score(now_ms);
    for (i, st) in stats.iter().enumerate() {
        if i == current {
            continue;
        }
        let s = st.score(now_ms);
        // require a clear win (> 1.5x + 1 B/s) to switch (anti-flapping)
        if s > best_score * 1.5 + 1.0 {
            best_score = s;
            best = i;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// S11-i B1: written-block ledger (which 256 KiB blocks are fully written,
// which are content-verified). In-memory only — NEVER persisted (the
// Android `.vdmctl` reader is strict-schema; spec §B1 non-goals).
// ---------------------------------------------------------------------------
struct AuditLedger {
    written: Mutex<BTreeSet<u64>>,
    audited: Mutex<BTreeSet<u64>>,
}

impl AuditLedger {
    fn new() -> Self {
        AuditLedger {
            written: Mutex::new(BTreeSet::new()),
            audited: Mutex::new(BTreeSet::new()),
        }
    }

    /// Insert every block index whose block lies entirely within
    /// `[start, end]` (partial straddling blocks become eligible when a
    /// neighboring lease completes them).
    fn mark_written(&self, start: u64, end: u64) {
        if end < start {
            return;
        }
        let first = start.div_ceil(AUDIT_BLOCK);
        let last = (end + 1) / AUDIT_BLOCK;
        if last == 0 || first >= last {
            return;
        }
        let mut w = self.written.lock().unwrap();
        for i in first..last {
            w.insert(i);
        }
    }

    fn mark_audited(&self, block: u64) {
        self.audited.lock().unwrap().insert(block);
    }

    /// Lowest written-but-unaudited block (deterministic anchor pick,
    /// spec §B1; late-written low blocks are still picked, and an
    /// inconclusive audit leaves its anchor as the lowest again).
    fn pick_auditable(&self) -> Option<u64> {
        let w = self.written.lock().unwrap();
        if w.is_empty() {
            return None;
        }
        let a = self.audited.lock().unwrap();
        w.iter().find(|b| !a.contains(b)).copied()
    }

    fn reset(&self) {
        self.written.lock().unwrap().clear();
        self.audited.lock().unwrap().clear();
    }
}

/// S11-i B2: signals for one optional-connection decision tick.
struct OptSignals {
    live: usize,
    speed: u64,
    one_conn_capacity: u64,
    remaining: u64,
}

#[derive(Debug)]
enum OptConnAction {
    Spawn,
    Retire,
    None,
}

/// S11-i B2: stateful spawn/retire controller. The 3-consecutive-tick
/// persistence lives here so the decision rules are unit-testable.
struct OptConnController {
    disabled: bool,
    cap: usize,
    extra: usize,
    spawn_ticks: u32,
    retire_ticks: u32,
}

impl OptConnController {
    fn new(
        _base_connections: usize,
        explicit_connections: usize,
        size: u64,
        rate_limited: bool,
        after_recovery: bool,
    ) -> Self {
        // absolute live cap: existing size-based amortization rule
        let cap = scale_connections(32, size);
        let disabled = size < OPT_MIN_SIZE
            || rate_limited
            || explicit_connections == 1 // user asked for single
            || after_recovery; // single-connection posture post-B1
        OptConnController {
            disabled,
            cap,
            extra: 0,
            spawn_ticks: 0,
            retire_ticks: 0,
        }
    }

    fn tick(&mut self, s: &OptSignals) -> OptConnAction {
        if self.disabled || s.live == 0 {
            return OptConnAction::None;
        }
        let live_f = s.live as f64;
        let capacity = s.one_conn_capacity as f64;
        let speed = s.speed as f64;
        let below = s.one_conn_capacity > 0
            && speed < OPT_SPAWN_HEADROOM * live_f * capacity
            && s.remaining >= OPT_MIN_REMAINING
            && s.live < self.cap
            && self.extra < OPT_EXTRA_MAX;
        if below {
            self.spawn_ticks += 1;
            self.retire_ticks = 0;
            if self.spawn_ticks >= OPT_TICKS_TO_ACT {
                self.spawn_ticks = 0;
                self.extra += 1;
                return OptConnAction::Spawn;
            }
            return OptConnAction::None;
        }
        self.spawn_ticks = 0;
        let saturated = s.live >= 2
            && s.one_conn_capacity > 0
            && speed >= OPT_RETIRE_HEADROOM * (live_f - 1.0) * capacity;
        let thin = s.remaining / (s.live as u64).max(1) < OPT_RETIRE_MIN_PER;
        if self.extra > 0 && (saturated || thin) {
            self.retire_ticks += 1;
            if self.retire_ticks >= OPT_TICKS_TO_ACT {
                self.retire_ticks = 0;
                self.extra -= 1;
                return OptConnAction::Retire;
            }
        } else {
            self.retire_ticks = 0;
        }
        OptConnAction::None
    }
}

/// Worker exit message: (worker id, exit kind) — S11-i monitor reaping.
type WorkerMsg = (usize, WorkerExit);

/// S11-i B1: audit one anchor block on its own thread (spec §B1
/// procedure). Every exit path clears the in-flight gate (RAII guard);
/// inconclusive audits change no ledger state.
#[allow(clippy::too_many_arguments)]
fn run_audit(
    block: u64,
    size: u64,
    file: &Arc<Mutex<std::fs::File>>,
    mirrors: &[String],
    cfg: &ClientCfg,
    ledger: &Arc<AuditLedger>,
    conflict: &Arc<AtomicBool>,
    mirror_rot: &Arc<AtomicU64>,
    in_flight: &Arc<AtomicBool>,
) {
    struct Gate<'a> {
        flag: &'a AtomicBool,
    }
    impl Drop for Gate<'_> {
        fn drop(&mut self) {
            self.flag.store(false, Ordering::Relaxed);
        }
    }
    let _gate = Gate { flag: in_flight };

    let start = block * AUDIT_BLOCK;
    let end = (start + AUDIT_BLOCK).min(size) - 1;
    let len = (end - start + 1) as usize;

    // 1. read the on-disk bytes under the file mutex (no torn reads)
    let mut disk = vec![0u8; len];
    {
        let f = file.lock().unwrap();
        if read_positioned(&f, start, &mut disk).is_err() {
            return; // inconclusive
        }
    }

    // 2. fresh connection; rotate mirrors deterministically
    let mi = (mirror_rot.fetch_add(1, Ordering::Relaxed) as usize) % mirrors.len().max(1);
    let url = match Url::parse(&mirrors[mi]) {
        Ok(u) => u,
        Err(_) => return,
    };
    let mut resp = match ranged_get(&url, cfg, start, end) {
        Ok(r) => r,
        Err(_) => return, // inconclusive
    };
    // S11-i B17: an invalid ranged response is INCONCLUSIVE for the audit —
    // never a B1 conflict signal, never trusted bytes.
    if classify_ranged_response(resp.status, &resp.headers, (start, end)).is_some() {
        let _ = resp.read_body(4096);
        return;
    }
    let full_file = start == 0 && end + 1 == size;
    if !(resp.status == 206 || (resp.status == 200 && full_file)) {
        let _ = resp.read_body(4096);
        return; // inconclusive
    }

    // 3. stream-compare (audit traffic bypasses the token buckets)
    let mut buf = vec![0u8; 64 * 1024];
    let mut checked = 0usize;
    let mut mismatch = false;
    loop {
        let mut br = resp.body_reader();
        let n = match Read::read(&mut br, &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return, // inconclusive; block stays unaudited
        };
        let take = n.min(len - checked);
        if disk[checked..checked + take] != buf[..take] {
            mismatch = true;
            break;
        }
        checked += take;
        if checked >= len || take < n {
            break;
        }
    }
    if checked < len && !mismatch {
        return; // short body — inconclusive
    }
    if mismatch {
        // wake the monitor for single-connection recovery
        conflict.store(true, Ordering::Release);
        return;
    }
    ledger.mark_audited(block);
}

/// Positioned read (pread semantics), mirroring `write_positioned`.
fn read_positioned(f: &std::fs::File, pos: u64, buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, pos)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut off = pos;
        let mut filled = 0usize;
        while filled < buf.len() {
            let n = f.seek_read(&mut buf[filled..], off)?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "read_positioned hit EOF",
                ));
            }
            filled += n;
            off += n as u64;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// S11-i B9: file-lock classification (spec §B9).
// ---------------------------------------------------------------------------

/// OS error codes meaning "another process holds the file" (per-OS
/// table). Shapes messaging and finalize retry only; never suppresses.
fn is_lock_conflict(e: &std::io::Error) -> bool {
    let code = e.raw_os_error();
    #[cfg(windows)]
    {
        matches!(code, Some(5) | Some(32) | Some(33))
    }
    #[cfg(unix)]
    {
        matches!(code, Some(1) | Some(13) | Some(16))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = code;
        false
    }
}

/// Finalize rename with bounded retry on lock-class conflicts (B9):
/// up to 3 attempts total at `delay` spacing; non-lock errors return
/// immediately.
fn finalize_with_retry<F: FnMut() -> std::io::Result<()>>(
    mut attempt: F,
    delay: Duration,
) -> std::io::Result<()> {
    let mut remaining = 2u32; // retries after a first failed attempt
    loop {
        match attempt() {
            Ok(()) => return Ok(()),
            Err(e) => {
                if remaining == 0 || !is_lock_conflict(&e) {
                    return Err(e);
                }
                remaining -= 1;
                std::thread::sleep(delay);
            }
        }
    }
}

/// Positioned write (pwrite semantics): one syscall, no cursor, safe under
/// the shared file lock by construction.
fn write_positioned(f: &std::fs::File, buf: &[u8], pos: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.write_all_at(buf, pos)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut off = pos;
        let mut slice = buf;
        while !slice.is_empty() {
            let n = f.seek_write(slice, off)?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "write_positioned wrote 0 bytes",
                ));
            }
            off += n as u64;
            slice = &slice[n..];
        }
        Ok(())
    }
}

/// Run one HTTP/HLS task to completion (called on its own thread).
pub fn run_http_task(
    id: u64,
    shared: Arc<EngineShared>,
    env: TaskEnv,
    control: Arc<crate::engine::events::TaskControl>,
    url: String,
    mut opts: HttpOptions,
) {
    // S11-i B16: resolve the effective task proxy once, at task creation —
    // suggest, never override. An explicit task proxy always wins; the
    // browser-reported proxy is adopted only when a probe reaches it (the
    // differs-warning was already surfaced at add time by the engine).
    let (resolved_proxy, proxy_notice) =
        resolve_task_proxy(opts.proxy.as_ref(), opts.browser_proxy.as_ref());
    if let Some(w) = proxy_notice {
        shared.update(id, |s| s.notice = Some(w.to_string()));
    }
    if resolved_proxy.is_some() {
        opts.proxy = resolved_proxy;
    }
    let cfg = ClientCfg {
        user_agent: opts
            .user_agent
            .clone()
            .unwrap_or_else(|| env.user_agent.clone()),
        extra_headers: opts.headers.clone(),
        proxy: opts.proxy.clone().or_else(|| env.proxy.clone()),
        connect_timeout: Some(Duration::from_secs(15)),
        io_timeout: Some(Duration::from_secs(30)),
    };
    let url_parsed = match Url::parse(&url) {
        Ok(u) => u,
        Err(e) => {
            fail(&shared, id, &e.to_string());
            return;
        }
    };

    shared.set_state(id, TaskState::Probing);
    // probe with retries + mirror failover at the probe stage
    let mut probed: Option<ProbeResult> = None;
    let mut last_err = None;
    let mut candidates: Vec<String> = std::iter::once(url.clone())
        .chain(opts.mirrors.iter().cloned())
        .collect();
    'probe: for candidate in candidates.drain(..) {
        for attempt in 0..5u32 {
            if control.is_stopped() || control.is_paused() {
                shared.set_state(id, TaskState::Paused);
                return;
            }
            let parsed = match Url::parse(&candidate) {
                Ok(u) => u,
                Err(e) => {
                    last_err = Some(e);
                    continue 'probe;
                }
            };
            match crate::engine::task::probe_with_meta(&parsed, &cfg) {
                Ok(p) => {
                    probed = Some(p);
                    break 'probe;
                }
                Err(pf) => {
                    // S11-i B21b: structural retry decision + bounded
                    // server-controlled wait (stop-responsive ticks).
                    let retry = pf.retryable();
                    let throttle = pf.throttle_wait();
                    last_err = Some(Error::Http(pf.msg));
                    if !retry {
                        continue 'probe; // try the next mirror immediately
                    }
                    match throttle {
                        Some(w) => stop_aware_sleep(&control, w),
                        None => {
                            std::thread::sleep(Duration::from_millis(400 * (attempt as u64 + 1)))
                        }
                    }
                }
            }
        }
    }
    let probe = match probed {
        Some(p) => p,
        None => {
            let msg = last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "probe failed".into());
            fail(&shared, id, &msg);
            return;
        }
    };

    let dir = opts
        .out_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| env.download_dir.clone());
    let _ = std::fs::create_dir_all(&dir);

    let is_hls = probe
        .content_type
        .as_deref()
        .map(|t| t.to_ascii_lowercase().contains("mpegurl"))
        .unwrap_or(false)
        || probe
            .final_url
            .path_and_query
            .to_ascii_lowercase()
            .ends_with(".m3u8");

    let filename = opts
        .filename
        .clone()
        .or_else(|| crate::util::filename_from_url(&url))
        .unwrap_or_else(|| probe.filename.clone());
    // S11-i B8: when the URL yields no usable extension and the probe knows
    // the content type, suggest one from our IANA-compiled MIME table. The
    // suggestion NEVER overrides an explicit user filename or an existing
    // extension; an unknown MIME type earns no suggestion (never a guess).
    let filename = match (
        opts.filename.is_none(),
        std::path::Path::new(&filename).extension().is_none(),
        probe.content_type.as_deref(),
    ) {
        (true, true, Some(ct)) => match crate::mime::extension_for_mime(ct) {
            Some(ext) => format!("{}.{}", filename.trim_end_matches('.'), ext),
            None => filename,
        },
        _ => filename,
    };
    let mut out_path = dir.join(crate::util::sanitize_filename(&filename));
    if is_hls {
        // assembled segment streams are MPEG-TS
        out_path = out_path.with_extension("ts");
    }

    if is_hls {
        run_hls(
            id,
            shared,
            env,
            control,
            &url_parsed,
            &opts,
            &out_path,
            &probe,
        );
        return;
    }
    if probe.size == 0 || !probe.accepts_ranges {
        // S11-i B21a (docs/SPECS/S11-ADOPTIONS-WAVE4.md; study §4d): a server
        // that never advertised range support is classified no-resume at the
        // PROBE (precedence over B18's changed-resource, which requires a
        // range-capable probe followed by a 200 at offset). A restart from
        // zero must be EXPLICIT when partial progress existed — never silent.
        let had_partial = !opts.fresh
            && (ResumeState::load(&out_path).is_some()
                || std::fs::metadata(ResumeState::part_path(&out_path))
                    .map(|m| m.len() > 0)
                    .unwrap_or(false));
        if had_partial {
            shared.update(id, |s| {
                s.notice = Some(NOTICE_NO_RESUME.to_string());
            });
        }
        run_single(id, shared, env, control, &url_parsed, &out_path);
        return;
    }
    run_segmented(
        id, shared, env, control, url_parsed, url, opts, out_path, probe,
    );
}

#[allow(clippy::too_many_arguments)]
fn run_segmented(
    id: u64,
    shared: Arc<EngineShared>,
    env: TaskEnv,
    control: Arc<crate::engine::events::TaskControl>,
    _url_parsed: Url,
    url: String,
    opts: HttpOptions,
    out_path: PathBuf,
    probe: ProbeResult,
) {
    // resume state reuse
    let mut state = if opts.fresh {
        None
    } else {
        ResumeState::load(&out_path)
    };
    state = state.filter(|st| {
        st.matches(
            &url,
            probe.size,
            probe.etag.as_deref(),
            probe.last_modified.as_deref(),
        ) && st.remaining.iter().all(|iv| iv.end < probe.size)
    });

    let connections = if opts.connections == 0 {
        env.default_connections
    } else {
        opts.connections.min(32)
    };
    // Koo et al.: parallel setup cost is only amortized when each
    // connection carries enough data; cap by file size (>= ~1 MiB per conn).
    let connections = scale_connections(connections, probe.size);
    // S11-i B2p: the preset is a CEILING, never a raise — effective base =
    // min(preset ceiling, size-based scale cap, absolute cap 32); `auto`
    // leaves the scaled base unchanged. The B2 spawn/retire machinery and
    // its own cap are untouched (presets bound the BASE only).
    let connections = apply_preset_ceiling(connections, &opts.conn_preset);
    // S11-i B18: in-memory If-Range validator for this run (never persisted;
    // ResumeState untouched — cross-restart validator persistence is a
    // recorded non-goal of wave 3).
    let if_range = if_range_validator(probe.etag.as_deref(), probe.last_modified.as_deref());
    // Mirror list: final probe URL first, then user mirrors, deduplicated.
    let mut mirrors: Vec<String> = vec![probe.final_url.to_string_full()];
    for m in opts.mirrors.iter() {
        if Url::parse(m).is_ok() && !mirrors.contains(m) {
            mirrors.push(m.clone());
        }
    }
    let mirror_stats: Arc<Vec<MirrorStat>> =
        Arc::new((0..mirrors.len()).map(|_| MirrorStat::new()).collect());

    let pool = match &state {
        Some(st) if !st.remaining.is_empty() => {
            RangePool::from_intervals(probe.size, st.remaining.clone())
        }
        _ => RangePool::new(probe.size),
    };
    let pool = Arc::new(pool);

    let part_path = ResumeState::part_path(&out_path);
    let file = match std::fs::OpenOptions::new()
        .write(true)
        .read(true)
        .create(true)
        .truncate(false) // resume: never clobber existing .part data
        .open(&part_path)
    {
        Ok(f) => f,
        Err(e) => {
            let msg = if is_lock_conflict(&e) {
                LOCK_WRITE_MSG.to_string()
            } else {
                format!("open part file: {}", e)
            };
            fail(&shared, id, &msg);
            return;
        }
    };
    if let Err(e) = file.set_len(probe.size) {
        let msg = if is_lock_conflict(&e) {
            LOCK_WRITE_MSG.to_string()
        } else {
            format!("preallocate: {}", e)
        };
        fail(&shared, id, &msg);
        return;
    }
    let file = Arc::new(Mutex::new(file));

    let saved = ResumeState {
        version: 1,
        url,
        mirrors: opts.mirrors.clone(),
        size: probe.size,
        etag: probe.etag.clone(),
        last_modified: probe.last_modified.clone(),
        checksum_algo: opts.checksum_algo.clone(),
        checksum_hex: opts.checksum_hex.clone(),
        filename: out_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default(),
        remaining: pool.intervals(),
    };
    let state_sync = Arc::new(Mutex::new(saved));

    let done = Arc::new(AtomicU64::new(probe.size - pool.remaining()));
    let task_bucket = Arc::new(TokenBucket::new(opts.rate_limit_bps, 256 * 1024));

    shared.update(id, |s| {
        s.size = probe.size;
        s.done = done.load(Ordering::Relaxed);
        s.connections = connections as u32;
        s.out_path = out_path.to_string_lossy().to_string();
        s.dir = out_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        s.name = out_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| s.name.clone());
    });
    shared.set_state(id, TaskState::Downloading);

    let (tx, rx): (std::sync::mpsc::Sender<WorkerMsg>, Receiver<WorkerMsg>) =
        std::sync::mpsc::channel();

    // One client config shared by all workers and audits of this task.
    let worker_cfg = ClientCfg {
        user_agent: opts
            .user_agent
            .clone()
            .unwrap_or_else(|| env.user_agent.clone()),
        extra_headers: opts.headers.clone(),
        proxy: opts.proxy.clone().or_else(|| env.proxy.clone()),
        connect_timeout: Some(Duration::from_secs(15)),
        io_timeout: Some(Duration::from_secs(30)),
    };

    // S11-i shared machinery (specs: docs/SPECS/S11-ADOPTIONS.md).
    let ledger = Arc::new(AuditLedger::new());
    let reset_flag = Arc::new(AtomicBool::new(false));
    let conflict = Arc::new(AtomicBool::new(false));
    let recoveries = Arc::new(AtomicU32::new(0));
    let audit_in_flight = Arc::new(AtomicBool::new(false));
    let audit_mirror_rot = Arc::new(AtomicU64::new(0));
    let retire_flags: Arc<Mutex<BTreeMap<usize, Arc<AtomicBool>>>> =
        Arc::new(Mutex::new(BTreeMap::new()));
    let live: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let next_wid = AtomicUsize::new(connections);
    let rate_limited = opts.rate_limit_bps > 0 || env.global_bucket.rate() > 0;
    let mut opt = OptConnController::new(
        connections,
        opts.connections,
        probe.size,
        rate_limited,
        false,
    );
    let audits_on = !opts.no_audit && probe.size >= AUDIT_STRIDE;
    let file_c = file.clone();

    // Uniform worker spawner: base workers (wid < connections), optional
    // workers (B2, retire-flagged) and recovery workers (single, base-like).
    let spawn_worker = |wid: usize, optional: bool| -> std::thread::JoinHandle<()> {
        let tx = tx.clone();
        let pool = pool.clone();
        let file = file.clone();
        let state_sync = state_sync.clone();
        let cfg = worker_cfg.clone();
        let if_range = if_range.clone();
        let mirrors = mirrors.clone();
        let mirror_stats = mirror_stats.clone();
        let done = done.clone();
        let control = control.clone();
        let task_bucket = task_bucket.clone();
        let global = env.global_bucket.clone();
        let size = probe.size;
        let out_path_c = out_path.clone();
        let ledger = ledger.clone();
        let reset_flag = reset_flag.clone();
        let workers = connections;
        let shared_for_worker = shared.clone();
        let task_id = id;
        let retire = if optional {
            let flag = Arc::new(AtomicBool::new(false));
            retire_flags.lock().unwrap().insert(wid, flag.clone());
            Some(flag)
        } else {
            None
        };
        live.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            let exit = worker(
                wid,
                pool,
                file,
                state_sync,
                cfg,
                if_range,
                mirrors,
                mirror_stats,
                workers,
                done,
                control,
                task_bucket,
                global,
                size,
                &out_path_c,
                ledger,
                reset_flag,
                retire,
                &shared_for_worker,
                task_id,
            );
            let _ = tx.send((wid, exit));
        })
    };

    let mut handles = Vec::new();
    for w in 0..connections {
        handles.push(spawn_worker(w, false));
    }

    // monitor loop: snapshot updates at ~10 Hz, periodic state saves,
    // S11-i B1 audit cadence + conflict recovery, B2 optional-connection
    // decisions at 1 Hz.
    let mut last_sample = (Instant::now(), done.load(Ordering::Relaxed));
    let mut speed_ema = 0u64;
    let mut failures: Vec<String> = Vec::new();
    let mut state_save_at = Instant::now();
    let mut decision_at = Instant::now();
    let mut last_audit_start_done = 0u64;
    let mut recovering = false;
    // B2: high-water mark of the best single-connection capacity any
    // mirror demonstrated. Using the CURRENT EMA would let a long crawl
    // drag it down until the spawn signal goes blind (observed on slow
    // CI runners); capacity the pipe has shown stays the reference.
    let mut one_conn_hwm: u64 = 0;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok((wid, e)) => {
                let l = live.fetch_sub(1, Ordering::SeqCst) - 1;
                retire_flags.lock().unwrap().remove(&wid);
                shared.update(id, |s| s.connections = l as u32);
                if let WorkerExit::Failed(msg) = e {
                    failures.push(msg);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
        }
        let d = done.load(Ordering::Relaxed);
        let now = Instant::now();
        if now.duration_since(last_sample.0).as_millis() >= 100 {
            // saturating: the B1 recovery reset can move `done` backwards
            // between samples; a backwards sample reads as 0 progress
            let inst = ((d.saturating_sub(last_sample.1)) as f64
                / now.duration_since(last_sample.0).as_secs_f64()) as u64;
            speed_ema = if speed_ema == 0 {
                inst
            } else {
                (speed_ema * 7 + inst) / 8
            };
            last_sample = (now, d);
            shared.update(id, |s| {
                s.done = d;
                s.speed = speed_ema;
            });
            shared
                .bus
                .publish(crate::engine::events::Event::TaskProgress { id });
        }
        if state_save_at.elapsed() >= Duration::from_secs(2) {
            state_save_at = Instant::now();
            let mut st = state_sync.lock().unwrap();
            st.remaining = pool.intervals();
            let _ = st.save(&out_path);
        }

        // --- B1: audit cadence (one anchor per AUDIT_STRIDE of progress) ---
        if audits_on
            && !recovering
            && !audit_in_flight.load(Ordering::Relaxed)
            && live.load(Ordering::SeqCst) > 0
            && d >= AUDIT_STRIDE
            && d.saturating_sub(last_audit_start_done) >= AUDIT_STRIDE
        {
            if let Some(block) = ledger.pick_auditable() {
                last_audit_start_done = d;
                audit_in_flight.store(true, Ordering::Relaxed);
                let file = file_c.clone();
                let mirrors_a = mirrors.clone();
                let cfg_a = worker_cfg.clone();
                let ledger = ledger.clone();
                let conflict = conflict.clone();
                let rot = audit_mirror_rot.clone();
                let gate = audit_in_flight.clone();
                let size = probe.size;
                std::thread::spawn(move || {
                    run_audit(
                        block, size, &file, &mirrors_a, &cfg_a, &ledger, &conflict, &rot, &gate,
                    );
                });
            }
        }

        // --- B1: conflict -> bounded single-connection recovery ---
        if conflict.load(Ordering::Acquire) && !recovering {
            let prev = recoveries.fetch_add(1, Ordering::SeqCst);
            if prev >= AUDIT_RECOVERIES_MAX {
                failures.push(
                    "repeated cross-connection data mismatches after recovery retries".into(),
                );
                reset_flag.store(true, Ordering::Relaxed); // tear the rest down
            } else {
                recovering = true;
                reset_flag.store(true, Ordering::Relaxed);
                shared.update(id, |s| {
                    s.notice = Some(NOTICE_RECOVERY.to_string());
                });
                failures.clear();
                speed_ema = 0;
                last_sample = (Instant::now(), 0);
                last_audit_start_done = 0;
                opt.disabled = true; // single-connection posture for the task
            }
        }
        if recovering {
            if live.load(Ordering::SeqCst) == 0 {
                // all workers torn down: rebuild for the single-connection retry
                pool.reset(probe.size);
                done.store(0, Ordering::SeqCst);
                ledger.reset();
                reset_flag.store(false, Ordering::Relaxed);
                conflict.store(false, Ordering::Relaxed);
                recovering = false;
                // the done counter just moved backwards: reset the speed
                // sampler too (a stale last_sample would underflow below)
                speed_ema = 0;
                last_sample = (Instant::now(), 0);
                handles.push(spawn_worker(next_wid.fetch_add(1, Ordering::SeqCst), false));
                shared.update(id, |s| s.connections = 1);
                let mut st = state_sync.lock().unwrap();
                st.remaining = pool.intervals();
                let _ = st.save(&out_path);
            }
        } else if live.load(Ordering::SeqCst) == 0 && !audit_in_flight.load(Ordering::Relaxed) {
            // all workers exited and no audit is mid-flight: the last
            // audit's verdict must land before finalizing (its mismatch
            // would otherwise arrive after the break — completion race).
            break;
        }

        // --- B2: optional-connection decisions at 1 Hz ---
        if decision_at.elapsed() >= Duration::from_secs(1) {
            decision_at = Instant::now();
            let l = live.load(Ordering::SeqCst);
            if !recovering && l > 0 {
                let one_conn = mirror_stats
                    .iter()
                    .map(|m| m.speed_ema.load(Ordering::Relaxed))
                    .max()
                    .unwrap_or(0);
                if one_conn > one_conn_hwm {
                    one_conn_hwm = one_conn;
                }
                let signals = OptSignals {
                    live: l,
                    speed: speed_ema,
                    one_conn_capacity: one_conn_hwm,
                    remaining: pool.remaining(),
                };
                if std::env::var_os("VDM_B2_DEBUG").is_some() {
                    eprintln!(
                        "DBG-B2 live={} speed={} hwm={} remaining={} disabled={}",
                        l, speed_ema, one_conn_hwm, signals.remaining, opt.disabled
                    );
                }
                let action = opt.tick(&signals);
                if std::env::var_os("VDM_B2_DEBUG").is_some() {
                    eprintln!("DBG-B2 action={:?}", action);
                }
                match action {
                    OptConnAction::Spawn => {
                        let wid = next_wid.fetch_add(1, Ordering::SeqCst);
                        handles.push(spawn_worker(wid, true));
                        shared.update(id, |s| {
                            s.connections = live.load(Ordering::SeqCst) as u32;
                        });
                    }
                    OptConnAction::Retire => {
                        // retire the highest-wid optional worker
                        let map = retire_flags.lock().unwrap();
                        if let Some((_, flag)) = map.iter().next_back() {
                            flag.store(true, Ordering::Relaxed);
                        }
                    }
                    OptConnAction::None => {}
                }
            }
        }
    }
    for h in handles {
        let _ = h.join();
    }

    let paused = control.is_paused();
    let stopped = control.is_stopped();
    {
        let mut st = state_sync.lock().unwrap();
        st.remaining = pool.intervals();
        let _ = st.save(&out_path);
    }

    if paused || stopped {
        shared.set_state(id, TaskState::Paused);
        return;
    }
    if let Some(err) = failures.first() {
        fail(&shared, id, err);
        return;
    }
    if !pool.is_empty() {
        fail(&shared, id, "download ended with missing ranges");
        return;
    }

    // checksum verification of the completed .part
    if let (Some(algo_s), Some(hex)) = (opts.checksum_algo.as_deref(), opts.checksum_hex.clone()) {
        shared.set_state(id, TaskState::Verifying);
        let algo = match crate::checksum::Algo::parse(algo_s) {
            Ok(a) => a,
            Err(e) => {
                fail(&shared, id, &e.to_string());
                return;
            }
        };
        match crate::checksum::verify_file(&part_path, algo, &hex) {
            Ok(true) => {
                shared.update(id, |s| s.checksum_ok = Some(true));
            }
            Ok(false) => {
                fail(&shared, id, "checksum mismatch");
                return;
            }
            Err(e) => {
                fail(&shared, id, &e.to_string());
                return;
            }
        }
    }

    if let Err(e) = finalize_with_retry(
        || std::fs::rename(&part_path, &out_path),
        Duration::from_secs(2),
    ) {
        let msg = if is_lock_conflict(&e) {
            LOCK_FINALIZE_MSG.to_string()
        } else {
            format!("finalize: {}", e)
        };
        fail(&shared, id, &msg);
        return;
    }
    let _ = std::fs::remove_file(ResumeState::ctl_path(&out_path));
    shared.update(id, |s| {
        s.done = s.size;
        s.speed = 0;
        // S11-i B16: transient recovery notices clear at completion, but a
        // persistent configuration warning (e.g. the browser's proxy was
        // unreachable and got dropped) stays visible
        s.notice = s.notice.take().filter(|n| n != NOTICE_RECOVERY);
    });
    shared.set_state(id, TaskState::Completed);
    shared
        .bus
        .publish(crate::engine::events::Event::TaskCompleted {
            id,
            path: out_path.to_string_lossy().to_string(),
        });
}

/// S11-i B20: first-write-wins notice when a response reports that a
/// request header value was sanitized (never clobbers an existing notice).
fn note_sanitized(shared: &Arc<EngineShared>, id: u64, resp: &http::Response) {
    if !resp.sanitized_headers {
        return;
    }
    shared.update(id, |s| {
        if s.notice.is_none() {
            s.notice = Some(NOTICE_SANITIZED.to_string());
        }
    });
}

fn fail(shared: &Arc<EngineShared>, id: u64, msg: &str) {
    shared.update(id, |s| {
        s.state = TaskState::Failed;
        s.error = Some(msg.to_string());
        s.speed = 0;
        // S11-i B19: refreshable failures surface an actionable hint on the
        // EXISTING note channel (rendering truth: the CLI prints notes; the
        // desktop frontends render `error` — the notice field is additive
        // and its frontend rendering is a recorded pre-existing gap, see
        // S11-ADOPTIONS-WAVE4 §B21 rule 8. No new surface, no new
        // permission, no RPC change).
        if msg.contains(REFRESHABLE_MARKER) {
            s.notice = Some(
                "This link looks expired or forbidden — open the source page, re-grab the link (extension grab-links or the page context), then add the fresh address."
                    .to_string(),
            );
        }
    });
    shared
        .bus
        .publish(crate::engine::events::Event::TaskFailed {
            id,
            error: msg.to_string(),
        });
}

/// Single-stream download for servers without range support / unknown size.
fn run_single(
    id: u64,
    shared: Arc<EngineShared>,
    env: TaskEnv,
    control: Arc<crate::engine::events::TaskControl>,
    url: &Url,
    out_path: &Path,
) {
    let cfg = ClientCfg {
        user_agent: env.user_agent.clone(),
        extra_headers: Vec::new(),
        proxy: env.proxy.clone(),
        connect_timeout: Some(Duration::from_secs(15)),
        io_timeout: Some(Duration::from_secs(30)),
    };
    let part = ResumeState::part_path(out_path);
    let mut resp = None;
    let mut last_err: Option<Error> = None;
    for attempt in 0..5u32 {
        if control.is_stopped() || control.is_paused() {
            shared.set_state(id, TaskState::Paused);
            return;
        }
        let attempt_result = (|| -> Result<http::Response> {
            let mut conn = Conn::connect(url, &cfg)?;
            conn.request(&Request::get(url), &cfg)
        })();
        let mut throttle: Option<Duration> = None;
        match attempt_result {
            Ok(r) if r.status == 200 => {
                resp = Some(r);
                break;
            }
            Ok(r) => {
                let status = r.status;
                // S11-i B21b: structural retry classification — 429/503/5xx
                // are retried (with the bounded server-controlled wait when
                // the server sent one). This resolves the pre-wave
                // inconsistency where this ladder never retried 5xx because
                // its formatted error string never matched `is_retryable`.
                throttle = retry_after_delay(status, parse_retry_after(r.header("retry-after")));
                last_err = Some(Error::Http(format!("server returned {}", status)));
                if !is_transient_status(status) {
                    break;
                }
            }
            Err(e) => {
                let retry = is_retryable(&e);
                last_err = Some(e);
                if !retry {
                    break;
                }
            }
        }
        match throttle {
            Some(w) => stop_aware_sleep(&control, w),
            None => std::thread::sleep(Duration::from_millis(400 * (attempt as u64 + 1))),
        }
    }
    let mut resp = match resp {
        Some(r) => r,
        None => {
            fail(
                &shared,
                id,
                &last_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "request failed".into()),
            );
            return;
        }
    };
    // S11-i B20: surface header-value sanitization once per task.
    note_sanitized(&shared, id, &resp);
    let size = resp.content_length().unwrap_or(0);
    shared.update(id, |s| {
        s.size = size;
        s.out_path = out_path.to_string_lossy().to_string();
        s.name = out_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| s.name.clone());
    });
    shared.set_state(id, TaskState::Downloading);
    let mut file = match std::fs::File::create(&part) {
        Ok(f) => f,
        Err(e) => {
            let msg = if is_lock_conflict(&e) {
                LOCK_WRITE_MSG.to_string()
            } else {
                e.to_string()
            };
            fail(&shared, id, &msg);
            return;
        }
    };
    // S11-i B21a coherency rule: the truncation just discarded the bytes the
    // matching `.vdmctl` describes — invalidate the stale sidecar (ctl only;
    // the open `.part` must not be unlinked) so a later resume against a
    // range-capable server cannot skip truncated ranges.
    ResumeState::remove_ctl(out_path);
    let done = AtomicU64::new(0);
    let mut buf = vec![0u8; 64 * 1024];
    let mut speed_ema = 0u64;
    let mut last = (Instant::now(), 0u64);
    loop {
        if control.is_stopped() || control.is_paused() {
            shared.set_state(id, TaskState::Paused);
            return;
        }
        let n = {
            let mut br = resp.body_reader();
            match Read::read(&mut br, &mut buf) {
                Ok(n) => n,
                Err(e) => {
                    fail(&shared, id, &format!("read: {}", e));
                    return;
                }
            }
        };
        if n == 0 {
            break;
        }
        env.global_bucket.wait_for(n as u64);
        if let Err(e) = file.write_all(&buf[..n]) {
            fail(&shared, id, &format!("write: {}", e));
            return;
        }
        let d = done.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
        let now = Instant::now();
        if now.duration_since(last.0).as_millis() >= 100 {
            let inst = ((d - last.1) as f64 / now.duration_since(last.0).as_secs_f64()) as u64;
            speed_ema = if speed_ema == 0 {
                inst
            } else {
                (speed_ema * 7 + inst) / 8
            };
            last = (now, d);
            shared.update(id, |s| {
                s.done = d;
                s.speed = speed_ema;
            });
        }
    }
    if let Err(e) = finalize_with_retry(|| std::fs::rename(&part, out_path), Duration::from_secs(2))
    {
        let msg = if is_lock_conflict(&e) {
            LOCK_FINALIZE_MSG.to_string()
        } else {
            format!("finalize: {}", e)
        };
        fail(&shared, id, &msg);
        return;
    }
    shared.update(id, |s| {
        s.done = s.size.max(s.done);
        s.speed = 0;
        // S11-i B16: transient recovery notices clear at completion, but a
        // persistent configuration warning stays visible
        s.notice = s.notice.take().filter(|n| n != NOTICE_RECOVERY);
    });
    shared.set_state(id, TaskState::Completed);
    shared
        .bus
        .publish(crate::engine::events::Event::TaskCompleted {
            id,
            path: out_path.to_string_lossy().to_string(),
        });
}

/// HLS task wrapper bridging control flags and progress into snapshots.
#[allow(clippy::too_many_arguments)]
fn run_hls(
    id: u64,
    shared: Arc<EngineShared>,
    env: TaskEnv,
    control: Arc<crate::engine::events::TaskControl>,
    url: &Url,
    opts: &HttpOptions,
    out_path: &Path,
    probe: &ProbeResult,
) {
    let cfg = ClientCfg {
        user_agent: opts
            .user_agent
            .clone()
            .unwrap_or_else(|| env.user_agent.clone()),
        extra_headers: opts.headers.clone(),
        proxy: opts.proxy.clone().or_else(|| env.proxy.clone()),
        connect_timeout: Some(Duration::from_secs(15)),
        io_timeout: Some(Duration::from_secs(30)),
    };
    let task_bucket = if opts.rate_limit_bps > 0 {
        Some(Arc::new(TokenBucket::new(opts.rate_limit_bps, 256 * 1024)))
    } else {
        None
    };
    let rate: Option<Arc<TokenBucket>> = if env.global_bucket.rate() > 0 {
        Some(env.global_bucket.clone())
    } else {
        task_bucket
    };
    let mut last = (Instant::now(), 0u64);
    let mut speed_ema = 0u64;
    shared.update(id, |s| {
        s.out_path = out_path.to_string_lossy().to_string();
        s.name = out_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| s.name.clone());
        s.size = probe.size;
    });
    shared.set_state(id, TaskState::Downloading);
    let shared2 = shared.clone();
    let mut progress = move |done: u64, total: u64| {
        let now = Instant::now();
        if now.duration_since(last.0).as_millis() >= 100 {
            let inst = ((done - last.1) as f64 / now.duration_since(last.0).as_secs_f64()) as u64;
            speed_ema = if speed_ema == 0 {
                inst
            } else {
                (speed_ema * 7 + inst) / 8
            };
            last = (now, done);
            shared2.update(id, |s| {
                s.done = done;
                s.size = total;
                s.speed = speed_ema;
            });
        }
    };
    let pause = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let control2 = control.clone();
    let pause_t = pause.clone();
    let stop_t = stop.clone();
    let bridger = std::thread::spawn(move || loop {
        if stop_t.load(Ordering::Relaxed) {
            break;
        }
        let p = control2.is_paused();
        pause_t.store(p, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(100));
    });
    let result = crate::engine::hls::download_hls(
        &cfg,
        url,
        if opts.quality.is_empty() {
            "best"
        } else {
            &opts.quality
        },
        4,
        out_path,
        rate.as_ref(),
        pause.as_ref(),
        stop.as_ref(),
        &mut progress,
    );
    stop.as_ref().store(true, Ordering::Relaxed);
    let _ = bridger.join();
    match result {
        Ok(()) => {
            shared.update(id, |s| {
                s.done = s.size;
                s.speed = 0;
            });
            shared.set_state(id, TaskState::Completed);
            shared
                .bus
                .publish(crate::engine::events::Event::TaskCompleted {
                    id,
                    path: out_path.to_string_lossy().to_string(),
                });
        }
        Err(e) => {
            if control.is_paused() || control.is_stopped() {
                shared.set_state(id, TaskState::Paused);
            } else {
                fail(&shared, id, &e.to_string());
            }
        }
    }
}

/// One segmented worker.
///
/// Paper-informed behavior (docs/ALGORITHMS.md "Research notes"):
/// - **Persistent connections**: the keep-alive `Conn` survives across
///   leases and is recycled via `Response::into_conn()` after a clean,
///   fully-drained body; a TCP/TLS handshake is paid only on the first
///   lease, on mirror switches, or after errors.
/// - **Throughput-weighted mirrors** (Rodriguez et al.): workers start
///   spread round-robin over the mirrors and re-evaluate the best-scoring
///   mirror at each lease boundary, abandoning slow mirrors for fast ones.
/// - **Guided leases** (Koo et al. / guided self-scheduling): lease size
///   scales with remaining work so the endgame tail is shared by all
///   workers instead of being ground through by one.
/// - **AIMD bursts** (Padhye-shaped): per-worker burst window grows
///   additively on clean progress and halves on read errors.
///
/// S11-i additions (docs/SPECS/S11-ADOPTIONS.md): fully-completed leases
/// feed the B1 written-block ledger; the B1 recovery reset and the B2
/// retire flag are honored at the existing control-check points.
#[allow(clippy::too_many_arguments)]
fn worker(
    wid: usize,
    pool: Arc<RangePool>,
    file: Arc<Mutex<std::fs::File>>,
    state: Arc<Mutex<ResumeState>>,
    cfg: ClientCfg,
    if_range: Option<String>,
    mirrors: Vec<String>,
    mirror_stats: Arc<Vec<MirrorStat>>,
    workers: usize,
    done: Arc<AtomicU64>,
    control: Arc<crate::engine::events::TaskControl>,
    task_bucket: Arc<TokenBucket>,
    global: Arc<TokenBucket>,
    size: u64,
    out_path: &Path,
    ledger: Arc<AuditLedger>,
    reset_flag: Arc<AtomicBool>,
    retire: Option<Arc<AtomicBool>>,
    shared: &Arc<EngineShared>,
    id: u64,
) -> WorkerExit {
    let mirrors_n = mirrors.len();
    let mut mirror_idx = wid % mirrors_n.max(1); // round-robin spread (Rodriguez et al.)
    let mut retries = 0u32;
    const MAX_RETRIES: u32 = 16;
    let mut window: u64 = 256 * 1024;
    let mut buf: Vec<u8> = vec![0u8; window as usize]; // hoisted across leases
                                                       // Stagger starts when running many connections: a synchronized SYN
                                                       // burst induces loss on constrained links (parallel-TCP measurements).
    if workers > 8 && wid > 0 {
        std::thread::sleep(Duration::from_millis(((wid as u64) * 15).min(300)));
    }
    // Keep-alive connection held across leases (None = must reconnect).
    let mut conn: Option<Conn> = None;
    // True while `conn` is a recycled connection whose next request is its
    // first; a failure of exactly that request is the classic idle-timeout
    // case and is retried once, free, on the same mirror.
    let mut conn_recycled = false;

    loop {
        if control.is_stopped() || control.is_paused() {
            return WorkerExit::Paused;
        }
        if reset_flag.load(Ordering::Relaxed) {
            return WorkerExit::Reset;
        }
        if let Some(r) = retire.as_ref() {
            if r.load(Ordering::Relaxed) {
                return WorkerExit::Retired;
            }
        }
        let lease_len = guided_lease(pool.remaining(), workers as u64);
        let lease = match pool.acquire(lease_len) {
            Some(l) => l,
            None => return WorkerExit::Done,
        };
        let (start, end) = lease;
        let lease_started = Instant::now();
        let mut pos = start;
        'attempt: loop {
            if control.is_stopped() || control.is_paused() {
                pool.release(pos, end);
                return WorkerExit::Paused;
            }
            if reset_flag.load(Ordering::Relaxed) {
                return WorkerExit::Reset;
            }
            if conn.is_none() {
                // choose the best mirror for a fresh connection
                mirror_idx = pick_mirror(&mirror_stats, mirror_idx, crate::util::now_ms());
                let mirror_url = match Url::parse(&mirrors[mirror_idx]) {
                    Ok(u) => u,
                    Err(e) => {
                        retries += 1;
                        if retries > MAX_RETRIES {
                            return WorkerExit::Failed(format!("bad mirror url: {}", e));
                        }
                        mirror_idx = (mirror_idx + 1) % mirrors_n;
                        continue 'attempt;
                    }
                };
                match Conn::connect(&mirror_url, &cfg) {
                    Ok(c) => {
                        conn = Some(c);
                        conn_recycled = false;
                    }
                    Err(e) => {
                        retries += 1;
                        mirror_stats[mirror_idx].record_fail();
                        if retries > MAX_RETRIES {
                            return WorkerExit::Failed(format!("connect: {}", e));
                        }
                        mirror_idx = (mirror_idx + 1) % mirrors_n;
                        std::thread::sleep(Duration::from_millis(backoff_ms(retries)));
                        continue 'attempt;
                    }
                }
            }
            let mirror_url = match Url::parse(&mirrors[mirror_idx]) {
                Ok(u) => u,
                Err(e) => {
                    conn = None;
                    retries += 1;
                    if retries > MAX_RETRIES {
                        return WorkerExit::Failed(format!("bad mirror url: {}", e));
                    }
                    mirror_idx = (mirror_idx + 1) % mirrors_n;
                    continue 'attempt;
                }
            };
            let mut req =
                Request::get(&mirror_url).header("Range", &format!("bytes={}-{}", pos, end));
            // S11-i B18: If-Range validator (RFC 9110 §13.1.5) — a compliant
            // server answers 200 when the resource changed since the probe,
            // 206 when these bytes are still the ones we probed.
            if let Some(v) = &if_range {
                req = req.header("If-Range", v);
            }
            // S11-i B17: ranged requests pin the identity coding so byte
            // offsets cannot shift under a content coding (RFC 9110
            // §12.5.3); a caller-chosen encoding always wins.
            if !has_accept_encoding(&cfg.extra_headers) && !has_accept_encoding(&req.headers) {
                req = req.header("Accept-Encoding", "identity");
            }
            let resp_result = conn.as_mut().unwrap().request(&req, &cfg);
            let mut resp = match resp_result {
                Ok(r) => r,
                Err(e) => {
                    let stale_keepalive = conn_recycled;
                    conn = None; // poisoned; drop it
                    if stale_keepalive {
                        // idle-timeout on a recycled connection: retry once
                        // on a fresh connection to the SAME mirror, free
                        conn_recycled = false;
                        continue 'attempt;
                    }
                    retries += 1;
                    mirror_stats[mirror_idx].record_fail();
                    if retries > MAX_RETRIES {
                        return WorkerExit::Failed(format!("request: {}", e));
                    }
                    mirror_idx = (mirror_idx + 1) % mirrors_n;
                    std::thread::sleep(Duration::from_millis(backoff_ms(retries)));
                    continue 'attempt;
                }
            };
            conn_recycled = false;
            // S11-i B20: surface header-value sanitization once per task.
            note_sanitized(shared, id, &resp);
            // S11-i B18: definitive changed-resource verdict — with a
            // validator sent, a 200 at pos>0 cannot heal; fail fast with an
            // honest reason instead of churning MAX_RETRIES across mirrors.
            if ranged_200_is_changed(resp.status, pos, if_range.is_some()) {
                let _ = resp.read_body(4096);
                return WorkerExit::Failed(RESOURCE_CHANGED_MSG.to_string());
            }
            // S11-i B17: reject invalid ranged responses before any byte is
            // trusted; they ride the existing bounded retry/mirror path with
            // a distinct reason and never widen the lease or trigger the B1
            // audit-conflict machinery.
            if let Some(inv) = classify_ranged_response(resp.status, &resp.headers, (pos, end)) {
                let code = resp.status;
                let _ = resp.read_body(4096);
                conn = resp.into_conn(); // drained; may still recycle
                retries += 1;
                mirror_stats[mirror_idx].record_fail();
                if retries > MAX_RETRIES {
                    return WorkerExit::Failed(format!("{} (HTTP {})", inv.reason(), code));
                }
                let next = (mirror_idx + 1) % mirrors_n;
                if next != mirror_idx {
                    mirror_idx = next;
                    conn = None; // conn belongs to the failed mirror
                }
                std::thread::sleep(Duration::from_millis(backoff_ms(retries)));
                continue 'attempt;
            }
            let ok_status =
                resp.status == 206 || (resp.status == 200 && pos == 0 && end + 1 == size);
            if !ok_status {
                let code = resp.status;
                // S11-i B21b: capture the server's wait hint before consuming
                // the response; the bounded wait replaces the ladder only
                // when the server explicitly throttled (429/503 + delta).
                let throttle =
                    retry_after_delay(code, parse_retry_after(resp.header("retry-after")));
                let _ = resp.read_body(4096);
                conn = resp.into_conn(); // drained error body may still recycle
                retries += 1;
                mirror_stats[mirror_idx].record_fail();
                if retries > MAX_RETRIES {
                    return WorkerExit::Failed(format!(
                        "server returned {}",
                        status_failure_reason(code)
                    ));
                }
                let next = (mirror_idx + 1) % mirrors_n;
                if next != mirror_idx {
                    mirror_idx = next;
                    conn = None; // conn belongs to the failed mirror
                }
                match throttle {
                    Some(w) => {
                        // B21b: stop-responsive bounded wait + the existing
                        // per-worker start stagger as jitter so a fleet of
                        // workers does not reconnect in one synchronized burst.
                        stop_aware_sleep(
                            &control,
                            w + Duration::from_millis(((wid as u64) * 15).min(300)),
                        );
                    }
                    None => std::thread::sleep(Duration::from_millis(backoff_ms(retries))),
                }
                continue 'attempt;
            }
            // stream the lease
            if buf.len() < window as usize {
                buf.resize(window.min(1024 * 1024) as usize, 0);
            }
            loop {
                if control.is_stopped() || control.is_paused() {
                    pool.release(pos, end);
                    return WorkerExit::Paused;
                }
                let n = {
                    let mut br = resp.body_reader();
                    match Read::read(&mut br, &mut buf) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) => {
                            pool.release(pos, end);
                            conn = None;
                            retries += 1;
                            window = (window / 2).max(64 * 1024);
                            mirror_stats[mirror_idx].record_fail();
                            if retries > MAX_RETRIES {
                                return WorkerExit::Failed(format!("read: {}", e));
                            }
                            mirror_idx = (mirror_idx + 1) % mirrors_n;
                            std::thread::sleep(Duration::from_millis(backoff_ms(retries)));
                            continue 'attempt;
                        }
                    }
                };
                // clamp to the lease (defensive against over-sending servers)
                let remaining = (end - pos + 1) as usize;
                let take = n.min(remaining);
                // Pace and write in <=64 KiB chunks, re-checking pause/stop
                // between chunks: under a hard limit a full-window take can
                // block for seconds inside the token bucket, and pause must
                // stay responsive (~250 ms) instead.
                let mut paced = 0usize;
                let mut aborted = false;
                while paced < take {
                    if control.is_stopped() || control.is_paused() {
                        aborted = true;
                        break;
                    }
                    if reset_flag.load(Ordering::Relaxed) {
                        return WorkerExit::Reset;
                    }
                    let chunk = (take - paced).min(64 * 1024);
                    task_bucket.wait_for(chunk as u64);
                    global.wait_for(chunk as u64);
                    {
                        let f = file.lock().unwrap();
                        if let Err(e) =
                            write_positioned(&f, &buf[paced..paced + chunk], pos + paced as u64)
                        {
                            let msg = if is_lock_conflict(&e) {
                                LOCK_WRITE_MSG.to_string()
                            } else {
                                format!("write: {}", e)
                            };
                            return WorkerExit::Failed(msg);
                        }
                    }
                    paced += chunk;
                    done.fetch_add(chunk as u64, Ordering::Relaxed);
                }
                pos += paced as u64;
                if aborted {
                    pool.release(pos, end);
                    return WorkerExit::Paused;
                }
                window = (window + 32 * 1024).min(1024 * 1024);
                if take < n || pos > end {
                    break;
                }
                if buf.len() < window as usize {
                    buf.resize(window.min(1024 * 1024) as usize, 0);
                }
            }
            if pos <= end {
                // short body: release the unconsumed tail, fail over
                pool.release(pos, end);
                conn = None; // body not fully drained; cannot recycle
                retries += 1;
                mirror_stats[mirror_idx].record_fail();
                if retries > MAX_RETRIES {
                    return WorkerExit::Failed("short body".into());
                }
                mirror_idx = (mirror_idx + 1) % mirrors_n;
                std::thread::sleep(Duration::from_millis(backoff_ms(retries)));
                continue 'attempt;
            }
            // lease complete: feed the B1 audit ledger before anything
            // else (the fully-written blocks become audit-eligible)
            ledger.mark_written(start, end);
            // recycle the cleanly drained connection
            conn = resp.into_conn();
            conn_recycled = conn.is_some();
            // measured per-lease throughput feeds the mirror selection
            let elapsed = lease_started.elapsed().as_secs_f64();
            let speed = ((end - start + 1) as f64 / elapsed.max(1e-6)) as u64;
            mirror_stats[mirror_idx].record_success(speed);
            // Rodriguez et al. reassignment: a much faster mirror wins the
            // next lease even at the cost of one fresh handshake
            if mirrors_n > 1 {
                let best = pick_mirror(&mirror_stats, mirror_idx, crate::util::now_ms());
                if best != mirror_idx {
                    mirror_idx = best;
                    conn = None;
                }
            }
            // checkpoint the state file
            {
                let mut st = state.lock().unwrap();
                st.remaining = pool.intervals();
                let _ = st.save(out_path);
            }
            retries = 0;
            break 'attempt;
        }
    }
}

fn backoff_ms(retries: u32) -> u64 {
    250u64.saturating_mul(1u64 << retries.min(4)).min(4000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_filenames() {
        assert_eq!(
            filename_from_disposition("attachment; filename=\"report.pdf\""),
            Some("report.pdf".to_string())
        );
        assert_eq!(
            filename_from_disposition("attachment; filename*=UTF-8''%E6%96%87%E4%BB%B6.zip"),
            Some("文件.zip".to_string())
        );
        assert_eq!(filename_from_disposition("inline"), None);
    }

    #[test]
    fn backoff_growth() {
        assert_eq!(backoff_ms(1), 500);
        assert_eq!(backoff_ms(4), 4000);
        assert_eq!(backoff_ms(10), 4000);
    }

    #[test]
    fn guided_lease_sizing() {
        // large file: capped at 16 MiB (few requests, amortized RTT)
        assert_eq!(guided_lease(8 * 1024 * 1024 * 1024, 8), LEASE_MAX);
        // mid file: guided self-scheduling, remaining / workers
        assert_eq!(guided_lease(64 * 1024 * 1024, 8), 8 * 1024 * 1024);
        // geometric shrink toward the endgame: tail shared by all workers
        assert_eq!(guided_lease(4 * 1024 * 1024, 8), 512 * 1024);
        assert_eq!(guided_lease(2 * 1024 * 1024, 8), LEASE_MIN);
        assert_eq!(guided_lease(0, 8), LEASE_MIN);
        // never below the floor even for tiny remainders
        assert_eq!(guided_lease(1, 1), LEASE_MIN);
    }

    #[test]
    fn connection_scaling_by_size() {
        // 2 MiB file never warrants 16 connections
        assert_eq!(scale_connections(16, 2 * 1024 * 1024), 2);
        // 5 MiB caps at 5; large files are uncapped
        assert_eq!(scale_connections(8, 5 * 1024 * 1024), 5);
        assert_eq!(scale_connections(32, 512 * 1024 * 1024), 32);
        // always at least one connection
        assert_eq!(scale_connections(8, 0), 1);
        assert_eq!(scale_connections(0, 1024 * 1024 * 1024), 1);
    }

    // ---- S11-i B2p: link-class connection presets ----

    #[test]
    fn preset_validation_accepts_known_classes_only() {
        for ok in ["", "auto", "high", "medium", "low", "HIGH", " Low "] {
            assert!(validate_conn_preset(ok).is_ok(), "{:?} rejected", ok);
        }
        // normalization
        assert_eq!(validate_conn_preset("HIGH").unwrap(), "high");
        // unknown class rejected with our message
        let err = validate_conn_preset("turbo").unwrap_err();
        assert!(err.to_string().contains("unknown connection preset"));
        assert!(validate_conn_preset("auto+").is_err());
    }

    #[test]
    fn preset_ceiling_interaction_with_scale_cap() {
        // the preset lowers a large size-scaled base (preset wins below the cap)
        assert_eq!(apply_preset_ceiling(32, "high"), 8);
        assert_eq!(apply_preset_ceiling(32, "medium"), 4);
        assert_eq!(apply_preset_ceiling(32, "low"), 2);
        // auto = today's behavior unchanged
        assert_eq!(apply_preset_ceiling(32, "auto"), 32);
        assert_eq!(apply_preset_ceiling(32, ""), 32);
        // a preset NEVER raises: a small file already below the ceiling stays
        let small_base = scale_connections(8, 2 * 1024 * 1024); // 2
        assert_eq!(apply_preset_ceiling(small_base, "high"), small_base);
        // end-to-end shape: min(preset, scale cap, 32)
        let scaled = scale_connections(32, 512 * 1024 * 1024); // 32
        assert_eq!(apply_preset_ceiling(scaled, "medium"), 4);
    }

    // ---- S11-i B16: browser-proxy propagation (engine side) ----

    fn http_proxy(host: &str, port: u16) -> crate::proxy::ProxyConfig {
        crate::proxy::ProxyConfig::Http {
            host: host.into(),
            port,
            user: None,
            pass: None,
        }
    }

    #[test]
    fn task_proxy_precedence_and_warnings() {
        // explicit task proxy always wins; a differing browser proxy warns
        let task = http_proxy("10.0.0.1", 8080);
        let browser = http_proxy("127.0.0.1", 1080);
        let (p, w) = resolve_task_proxy(Some(&task), Some(&browser));
        assert_eq!(p, Some(task.clone()));
        assert_eq!(w, Some(WARN_PROXY_DIFFERS));
        // the SAME proxy does not warn
        let (_, w2) = resolve_task_proxy(Some(&browser), Some(&browser));
        assert_eq!(w2, None);
        // no browser proxy reported: nothing to resolve or warn
        let (p3, w3) = resolve_task_proxy(Some(&task), None);
        assert_eq!(p3, Some(task));
        assert_eq!(w3, None);
        let (p4, w4) = resolve_task_proxy(None, None);
        assert_eq!(p4, None);
        assert_eq!(w4, None);
    }

    #[test]
    fn browser_proxy_adopted_only_when_reachable() {
        // a live listener: the reported proxy is adopted (suggest, never
        // override — no explicit task proxy)
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let live = http_proxy("127.0.0.1", port);
        let (p, w) = resolve_task_proxy(None, Some(&live));
        assert_eq!(p, Some(live.clone()));
        assert_eq!(w, None);
        // an unreachable reported proxy is DROPPED with the warning
        let dead = http_proxy("127.0.0.1", 1);
        let (p2, w2) = resolve_task_proxy(None, Some(&dead));
        assert_eq!(p2, None);
        assert_eq!(w2, Some(WARN_PROXY_UNREACHABLE));
        // ...and an explicit task proxy still wins over a live browser proxy
        let task = http_proxy("10.9.9.9", 3128);
        let (p3, _) = resolve_task_proxy(Some(&task), Some(&live));
        assert_eq!(p3, Some(task));
    }

    #[test]
    fn mirror_scoring_prefers_fast_healthy_mirror() {
        let now = crate::util::now_ms();
        let slow = MirrorStat::new();
        slow.record_success(100_000);
        let fast = MirrorStat::new();
        fast.record_success(5_000_000);
        // fast wins from either perspective
        assert_eq!(pick_mirror(&[slow.clone(), fast.clone()], 0, now), 1);
        assert_eq!(pick_mirror(&[slow.clone(), fast.clone()], 1, now), 1);
        // a similar-speed mirror does NOT evict the current one (anti-flap)
        let other = MirrorStat::new();
        other.record_success(120_000);
        assert_eq!(pick_mirror(&[slow.clone(), other], 0, now), 0);
    }

    #[test]
    fn mirror_cooldown_blocks_selection() {
        let now = crate::util::now_ms();
        let dead = MirrorStat::new();
        dead.record_success(9_000_000);
        dead.record_fail(); // enters cooldown
        let alive = MirrorStat::new();
        alive.record_success(1_000);
        // even though `dead` is 9000x faster measured, cooldown wins
        assert_eq!(pick_mirror(&[dead, alive], 1, now), 1);
        // failures decay the score multiplicatively
        let flaky = MirrorStat::new();
        flaky.record_success(10_000_000);
        flaky.fails.store(3, Ordering::Relaxed);
        let steady = MirrorStat::new();
        steady.record_success(1_000_000);
        assert_eq!(pick_mirror(&[flaky, steady], 0, now), 1);
    }

    // ---------------------------------------------------------------------
    // S11-i wave 1 units (specs: docs/SPECS/S11-ADOPTIONS.md)
    // ---------------------------------------------------------------------

    #[test]
    fn audit_ledger_straddling_and_lowest_pick() {
        let l = AuditLedger::new();
        assert_eq!(l.pick_auditable(), None);
        // lease [100K, 600K): only block 1 [256K, 512K) fully inside
        l.mark_written(100 * 1024, 600 * 1024);
        assert_eq!(l.pick_auditable(), Some(1));
        // completing block 2 via [512K, 768K) makes 2 eligible; 1 stays lowest
        l.mark_written(512 * 1024, 768 * 1024);
        assert_eq!(l.pick_auditable(), Some(1));
        l.mark_audited(1);
        assert_eq!(l.pick_auditable(), Some(2));
        l.mark_audited(2);
        assert_eq!(l.pick_auditable(), None);
        // blocks 0 and 3 arrive later; 0 is below the cursor yet unaudited
        // -> still picked (deterministic lowest)
        l.mark_written(0, 256 * 1024);
        assert_eq!(l.pick_auditable(), Some(0));
    }

    #[test]
    fn audit_ledger_reset_clears() {
        let l = AuditLedger::new();
        l.mark_written(0, 2 * 1024 * 1024);
        l.mark_audited(3);
        assert!(l.pick_auditable().is_some());
        l.reset();
        assert_eq!(l.pick_auditable(), None);
        // written-set is empty too: new writes repopulate from scratch
        l.mark_written(0, 256 * 1024);
        assert_eq!(l.pick_auditable(), Some(0));
    }

    #[test]
    #[cfg(unix)]
    fn lock_classifier_unix_table() {
        let mk = |code: i32| std::io::Error::from_raw_os_error(code);
        assert!(is_lock_conflict(&mk(13))); // EACCES
        assert!(is_lock_conflict(&mk(16))); // EBUSY
        assert!(is_lock_conflict(&mk(1))); // EPERM
        assert!(!is_lock_conflict(&mk(32))); // EPIPE on unix — not a lock
        assert!(!is_lock_conflict(&mk(2))); // ENOENT
        assert!(!is_lock_conflict(&std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "not os"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn lock_classifier_windows_table() {
        let mk = |code: i32| std::io::Error::from_raw_os_error(code);
        assert!(is_lock_conflict(&mk(5))); // ERROR_ACCESS_DENIED
        assert!(is_lock_conflict(&mk(32))); // ERROR_SHARING_VIOLATION
        assert!(is_lock_conflict(&mk(33))); // ERROR_LOCK_VIOLATION
        assert!(!is_lock_conflict(&mk(2))); // file not found
        assert!(!is_lock_conflict(&mk(3))); // path not found
    }

    fn lock_err() -> std::io::Error {
        // a code that is lock-class on the target OS
        #[cfg(unix)]
        {
            std::io::Error::from_raw_os_error(13) // EACCES
        }
        #[cfg(windows)]
        {
            std::io::Error::from_raw_os_error(32) // ERROR_SHARING_VIOLATION
        }
        #[cfg(not(any(unix, windows)))]
        {
            std::io::Error::new(std::io::ErrorKind::Other, "no os codes")
        }
    }

    #[test]
    fn finalize_retry_bounded_on_locks() {
        // persistent lock: exactly 3 attempts total
        let mut calls = 0u32;
        let r = finalize_with_retry(
            || {
                calls += 1;
                Err::<(), _>(lock_err())
            },
            Duration::ZERO,
        );
        assert!(r.is_err());
        assert_eq!(calls, 3);
        // lock clears on attempt 2
        let mut calls2 = 0u32;
        let r2 = finalize_with_retry(
            || {
                calls2 += 1;
                if calls2 == 1 {
                    Err::<(), _>(lock_err())
                } else {
                    Ok(())
                }
            },
            Duration::ZERO,
        );
        assert!(r2.is_ok());
        assert_eq!(calls2, 2);
        // non-lock errors: single attempt, no retry
        let mut calls3 = 0u32;
        let r3 = finalize_with_retry(
            || {
                calls3 += 1;
                Err::<(), _>(std::io::Error::from_raw_os_error(2))
            },
            Duration::ZERO,
        );
        assert!(r3.is_err());
        assert_eq!(calls3, 1);
    }

    fn opt_signals(live: usize, speed: u64, cap1: u64, remaining: u64) -> OptSignals {
        OptSignals {
            live,
            speed,
            one_conn_capacity: cap1,
            remaining,
        }
    }

    #[test]
    fn opt_conn_eligibility_guards() {
        // too small -> disabled
        let mut c = OptConnController::new(4, 0, 15 * 1024 * 1024, false, false);
        assert!(matches!(
            c.tick(&opt_signals(4, 1, 1_000_000, 100 * 1024 * 1024)),
            OptConnAction::None
        ));
        // rate-limited -> disabled
        let mut c = OptConnController::new(4, 0, 100 * 1024 * 1024, true, false);
        assert!(matches!(
            c.tick(&opt_signals(4, 1, 1_000_000, 100 * 1024 * 1024)),
            OptConnAction::None
        ));
        // explicit single connection -> disabled (respect user intent)
        let mut c = OptConnController::new(1, 1, 100 * 1024 * 1024, false, false);
        assert!(matches!(
            c.tick(&opt_signals(1, 0, 1_000_000, 100 * 1024 * 1024)),
            OptConnAction::None
        ));
        // after a B1 recovery -> disabled
        let mut c = OptConnController::new(4, 0, 100 * 1024 * 1024, false, true);
        assert!(matches!(
            c.tick(&opt_signals(4, 1, 1_000_000, 100 * 1024 * 1024)),
            OptConnAction::None
        ));
    }

    #[test]
    fn opt_conn_spawn_needs_three_consecutive_ticks() {
        let mut c = OptConnController::new(2, 2, 100 * 1024 * 1024, false, false);
        let below = opt_signals(2, 1_000_000, 10_000_000, 100 * 1024 * 1024);
        assert!(matches!(c.tick(&below), OptConnAction::None));
        assert!(matches!(c.tick(&below), OptConnAction::None));
        // an above-threshold tick resets persistence
        let above = opt_signals(2, 20_000_000, 10_000_000, 100 * 1024 * 1024);
        assert!(matches!(c.tick(&above), OptConnAction::None));
        assert!(matches!(c.tick(&below), OptConnAction::None));
        assert!(matches!(c.tick(&below), OptConnAction::None));
        // third consecutive below-tick spawns
        assert!(matches!(c.tick(&below), OptConnAction::Spawn));
        // ...and resets the counter (next spawn needs 3 more)
        assert!(matches!(c.tick(&below), OptConnAction::None));
    }

    #[test]
    fn opt_conn_spawn_guards() {
        // remaining below 8 MiB: never spawn
        let mut c = OptConnController::new(2, 2, 100 * 1024 * 1024, false, false);
        let s = opt_signals(2, 1_000_000, 10_000_000, 4 * 1024 * 1024);
        for _ in 0..5 {
            assert!(matches!(c.tick(&s), OptConnAction::None));
        }
        // extra cap of 8: exhausted -> no spawn
        let mut c = OptConnController::new(2, 2, 100 * 1024 * 1024, false, false);
        c.extra = OPT_EXTRA_MAX;
        let s = opt_signals(2, 1_000_000, 10_000_000, 100 * 1024 * 1024);
        for _ in 0..5 {
            assert!(matches!(c.tick(&s), OptConnAction::None));
        }
    }

    #[test]
    fn opt_conn_retire_rules() {
        // saturation: aggregate >= 0.85 * (live-1) * capacity for 3 ticks
        let mut c = OptConnController::new(2, 2, 100 * 1024 * 1024, false, false);
        c.extra = 1;
        let sat = opt_signals(3, 18_000_000, 10_000_000, 100 * 1024 * 1024);
        assert!(matches!(c.tick(&sat), OptConnAction::None));
        assert!(matches!(c.tick(&sat), OptConnAction::None));
        assert!(matches!(c.tick(&sat), OptConnAction::Retire));
        // thin remaining (below the spawn floor so retire is reachable):
        // retire after the tick persistence
        let mut c = OptConnController::new(2, 2, 100 * 1024 * 1024, false, false);
        c.extra = 1;
        let thin = opt_signals(3, 1_000_000, 10_000_000, 7 * 1024 * 1024);
        assert!(matches!(c.tick(&thin), OptConnAction::None));
        assert!(matches!(c.tick(&thin), OptConnAction::None));
        assert!(matches!(c.tick(&thin), OptConnAction::Retire));
        // no extras: never retire
        let mut c = OptConnController::new(2, 2, 100 * 1024 * 1024, false, false);
        let sat2 = opt_signals(3, 18_000_000, 10_000_000, 100 * 1024 * 1024);
        for _ in 0..5 {
            assert!(matches!(c.tick(&sat2), OptConnAction::None));
        }
    }

    impl Clone for MirrorStat {
        fn clone(&self) -> Self {
            MirrorStat {
                speed_ema: AtomicU64::new(self.speed_ema.load(Ordering::Relaxed)),
                fails: AtomicU32::new(self.fails.load(Ordering::Relaxed)),
                cooldown_until: AtomicU64::new(self.cooldown_until.load(Ordering::Relaxed)),
                leases: AtomicU64::new(self.leases.load(Ordering::Relaxed)),
            }
        }
    }

    // ---- S11-i B17/B18/B19 (docs/SPECS/S11-ADOPTIONS-WAVE3.md) ----

    #[test]
    fn accept_encoding_detection_is_case_insensitive() {
        assert!(!has_accept_encoding(&[]));
        assert!(!has_accept_encoding(&[(
            "Accept".to_string(),
            "*".to_string()
        )]));
        assert!(has_accept_encoding(&[(
            "Accept-Encoding".to_string(),
            "gzip".to_string()
        )]));
        assert!(has_accept_encoding(&[(
            "ACCEPT-ENCODING".to_string(),
            "identity".to_string()
        )]));
    }

    #[test]
    fn content_range_parse_matrix() {
        assert_eq!(parse_content_range("bytes 0-99/500"), Some((0, 99)));
        assert_eq!(parse_content_range("BYTES 10-20/*"), Some((10, 20)));
        assert_eq!(parse_content_range("bytes */500"), None);
        assert_eq!(parse_content_range("bytes 5-4/10"), Some((5, 4)));
        assert_eq!(parse_content_range("chunky 1-2/3"), None);
        assert_eq!(parse_content_range(""), None);
    }

    #[test]
    fn ranged_response_invariant_matrix() {
        let want = (100u64, 199u64);
        let ok = vec![
            (
                "Content-Range".to_string(),
                "bytes 100-199/1000".to_string(),
            ),
            (
                "Content-Type".to_string(),
                "application/octet-stream".to_string(),
            ),
        ];
        assert_eq!(classify_ranged_response(206, &ok, want), None);
        // a coded body announcement invalidates the response
        let gz = vec![
            (
                "Content-Range".to_string(),
                "bytes 100-199/1000".to_string(),
            ),
            ("Content-Encoding".to_string(), "gzip".to_string()),
        ];
        assert_eq!(
            classify_ranged_response(206, &gz, want),
            Some(RangedInvalid::Compressed)
        );
        assert_eq!(
            classify_ranged_response(200, &gz, want),
            Some(RangedInvalid::Compressed)
        );
        // the multipart family (both observed spellings)
        for ct in ["multipart/byteranges; boundary=x", "multipart/x-byteranges"] {
            let mp = vec![
                (
                    "Content-Range".to_string(),
                    "bytes 100-199/1000".to_string(),
                ),
                ("Content-Type".to_string(), ct.to_string()),
            ];
            assert_eq!(
                classify_ranged_response(206, &mp, want),
                Some(RangedInvalid::Multipart)
            );
        }
        // content-range must match the requested range exactly on a 206
        let shifted = vec![("Content-Range".to_string(), "bytes 99-198/1000".to_string())];
        assert_eq!(
            classify_ranged_response(206, &shifted, want),
            Some(RangedInvalid::ContentRangeMismatch)
        );
        let short = vec![(
            "Content-Range".to_string(),
            "bytes 100-150/1000".to_string(),
        )];
        assert_eq!(
            classify_ranged_response(206, &short, want),
            Some(RangedInvalid::ContentRangeMismatch)
        );
        assert_eq!(
            classify_ranged_response(206, &[], want),
            Some(RangedInvalid::ContentRangeMismatch)
        );
        let unsatisfiable = vec![("Content-Range".to_string(), "bytes */1000".to_string())];
        assert_eq!(
            classify_ranged_response(206, &unsatisfiable, want),
            Some(RangedInvalid::ContentRangeMismatch)
        );
        // a plain 200 to a ranged request is NOT classified here (the
        // worker's ok_status / B18 changed-resource rules own that case)
        assert_eq!(classify_ranged_response(200, &ok, want), None);
    }

    #[test]
    fn if_range_validator_precedence() {
        assert_eq!(
            if_range_validator(Some("\"abc\""), Some("yesterday")),
            Some("\"abc\"".to_string())
        );
        assert_eq!(
            if_range_validator(None, Some("yesterday")),
            Some("yesterday".to_string())
        );
        assert_eq!(if_range_validator(None, None), None);
    }

    #[test]
    fn resource_changed_fast_fail_decision() {
        assert!(ranged_200_is_changed(200, 1, true));
        assert!(!ranged_200_is_changed(200, 0, true));
        assert!(!ranged_200_is_changed(200, 1, false));
        assert!(!ranged_200_is_changed(206, 1, true));
    }
    #[test]
    fn refreshable_status_classification() {
        for s in [403u16, 410] {
            let msg = status_failure_reason(s);
            assert!(msg.contains(REFRESHABLE_MARKER), "{}", msg);
            assert!(msg.starts_with("HTTP "));
        }
        // other statuses keep their historical exact strings
        assert_eq!(status_failure_reason(404), "HTTP 404");
        assert_eq!(status_failure_reason(500), "HTTP 500");
        assert_eq!(status_failure_reason(503), "HTTP 503");
        assert!(is_retryable(&Error::Http("HTTP 503".into())));
        assert!(!is_retryable(&Error::Http(status_failure_reason(403))));
    }

    // S11-i B21b: deterministic parse + delay matrices (no wall-clock CI).

    #[test]
    fn b21_retry_after_parse_and_delay() {
        // delta-seconds only
        assert_eq!(parse_retry_after(Some("2")), Some(Duration::from_secs(2)));
        assert_eq!(parse_retry_after(Some("0")), Some(Duration::from_secs(0)));
        assert_eq!(parse_retry_after(Some(" 3 ")), Some(Duration::from_secs(3)));
        // HTTP-date form: None (no date-parsing dependency)
        assert_eq!(
            parse_retry_after(Some("Mon, 02 Jan 2006 15:04:05 GMT")),
            None
        );
        // invalid shapes
        assert_eq!(parse_retry_after(Some("abc")), None);
        assert_eq!(parse_retry_after(Some("")), None);
        assert_eq!(parse_retry_after(Some("-5")), None);
        assert_eq!(parse_retry_after(Some("1234567")), None); // > 6 digits
        assert_eq!(parse_retry_after(None), None);
        // delay matrix: bounded server-controlled wait on 429/503 only
        assert_eq!(
            retry_after_delay(429, Some(Duration::from_secs(2))),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            retry_after_delay(503, Some(Duration::from_secs(120))),
            Some(RETRY_AFTER_CAP)
        );
        assert_eq!(retry_after_delay(503, None), None);
        assert_eq!(retry_after_delay(404, Some(Duration::from_secs(2))), None);
        // structural transient classification
        for s in [429u16, 500, 502, 503, 504] {
            assert!(is_transient_status(s), "{}", s);
        }
        for s in [200u16, 206, 403, 404, 410] {
            assert!(!is_transient_status(s), "{}", s);
        }
        // probe failure meta: status-bearing failures classify structurally
        let pf = ProbeFailure {
            msg: status_failure_reason(429),
            status: Some(429),
            retry_after: Some(Duration::from_secs(2)),
        };
        assert!(pf.retryable());
        assert_eq!(pf.throttle_wait(), Some(Duration::from_secs(2)));
        let pf403 = ProbeFailure {
            msg: status_failure_reason(403),
            status: Some(403),
            retry_after: None,
        };
        assert!(!pf403.retryable());
        assert_eq!(pf403.throttle_wait(), None);
    }
}
