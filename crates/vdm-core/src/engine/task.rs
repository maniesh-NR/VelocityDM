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
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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
    /// Do not resume: discard any `.part` state.
    #[serde(default)]
    pub fresh: bool,
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

/// HEAD (fallback ranged GET) probe with redirect resolution.
pub fn probe(url: &Url, cfg: &ClientCfg) -> Result<ProbeResult> {
    let (final_url, mut resp) = match http::resolve_redirects(url, cfg, 5, "HEAD") {
        Ok(v) => v,
        Err(_) => http::resolve_redirects(url, cfg, 5, "GET")?,
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
    let mut fatal: Option<String> = if head_status >= 400 {
        Some(format!("HTTP {}", head_status))
    } else {
        None
    };
    if !head_ok || size == 0 || !accepts_ranges {
        match ranged_get(&final_url, cfg, 0, 0) {
            Ok(mut r2) => {
                if r2.status >= 400 {
                    if fatal.is_none() {
                        fatal = Some(format!("HTTP {}", r2.status));
                    }
                } else {
                    fatal = None;
                    if r2.status == 206 {
                        accepts_ranges = true;
                        if let Some(cr) = r2.header("content-range") {
                            if let Some(total) = cr.rsplit('/').next() {
                                size = total.trim().parse().unwrap_or(size);
                            }
                        }
                    } else if size == 0 {
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
    if let Some(msg) = fatal {
        return Err(Error::Http(msg));
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

fn ranged_get(url: &Url, cfg: &ClientCfg, start: u64, end: u64) -> Result<http::Response> {
    let req = Request::get(url).header("Range", &format!("bytes={}-{}", start, end));
    let mut conn = Conn::connect(url, cfg)?;
    conn.request(&req, cfg)
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
    opts: HttpOptions,
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
            match crate::engine::task::probe(&parsed, &cfg) {
                Ok(p) => {
                    probed = Some(p);
                    break 'probe;
                }
                Err(e) => {
                    let retry = is_retryable(&e);
                    last_err = Some(e);
                    if !retry {
                        continue 'probe; // try the next mirror immediately
                    }
                    std::thread::sleep(Duration::from_millis(400 * (attempt as u64 + 1)));
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
            fail(&shared, id, &format!("open part file: {}", e));
            return;
        }
    };
    if let Err(e) = file.set_len(probe.size) {
        fail(&shared, id, &format!("preallocate: {}", e));
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

    let (tx, rx): (std::sync::mpsc::Sender<WorkerExit>, Receiver<WorkerExit>) =
        std::sync::mpsc::channel();
    let mut handles = Vec::new();
    for w in 0..connections {
        let tx = tx.clone();
        let pool = pool.clone();
        let file = file.clone();
        let state_sync = state_sync.clone();
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
        let mirrors = mirrors.clone();
        let mirror_stats = mirror_stats.clone();
        let done = done.clone();
        let control = control.clone();
        let task_bucket = task_bucket.clone();
        let global = env.global_bucket.clone();
        let size = probe.size;
        let out_path_c = out_path.clone();
        let workers = connections;
        handles.push(std::thread::spawn(move || {
            let exit = worker(
                w,
                pool,
                file,
                state_sync,
                cfg,
                mirrors,
                mirror_stats,
                workers,
                done,
                control,
                task_bucket,
                global,
                size,
                &out_path_c,
            );
            let _ = tx.send(exit);
        }));
    }
    drop(tx);

    // monitor loop: snapshot updates at ~10 Hz, periodic state saves
    let mut last_sample = (Instant::now(), done.load(Ordering::Relaxed));
    let mut speed_ema = 0u64;
    let mut exits: Vec<WorkerExit> = Vec::new();
    let mut state_save_at = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(e) => {
                exits.push(e);
                if exits.len() >= connections {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if exits.len() >= connections {
                    break;
                }
                // workers gone without reporting; stop waiting
                break;
            }
        }
        let d = done.load(Ordering::Relaxed);
        let now = Instant::now();
        if now.duration_since(last_sample.0).as_millis() >= 100 {
            let inst = ((d - last_sample.1) as f64
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
    if let Some(err) = exits.iter().find_map(|e| match e {
        WorkerExit::Failed(msg) => Some(msg.clone()),
        _ => None,
    }) {
        fail(&shared, id, &err);
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

    if let Err(e) = std::fs::rename(&part_path, &out_path) {
        fail(&shared, id, &format!("finalize: {}", e));
        return;
    }
    let _ = std::fs::remove_file(ResumeState::ctl_path(&out_path));
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

fn fail(shared: &Arc<EngineShared>, id: u64, msg: &str) {
    shared.update(id, |s| {
        s.state = TaskState::Failed;
        s.error = Some(msg.to_string());
        s.speed = 0;
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
        match attempt_result {
            Ok(r) if r.status == 200 => {
                resp = Some(r);
                break;
            }
            Ok(r) => {
                let e = Error::Http(format!("server returned {}", r.status));
                let retry = is_retryable(&e);
                last_err = Some(e);
                if !retry {
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
        std::thread::sleep(Duration::from_millis(400 * (attempt as u64 + 1)));
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
            fail(&shared, id, &e.to_string());
            return;
        }
    };
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
    if let Err(e) = std::fs::rename(&part, out_path) {
        fail(&shared, id, &format!("finalize: {}", e));
        return;
    }
    shared.update(id, |s| {
        s.done = s.size.max(s.done);
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
#[allow(clippy::too_many_arguments)]
fn worker(
    wid: usize,
    pool: Arc<RangePool>,
    file: Arc<Mutex<std::fs::File>>,
    state: Arc<Mutex<ResumeState>>,
    cfg: ClientCfg,
    mirrors: Vec<String>,
    mirror_stats: Arc<Vec<MirrorStat>>,
    workers: usize,
    done: Arc<AtomicU64>,
    control: Arc<crate::engine::events::TaskControl>,
    task_bucket: Arc<TokenBucket>,
    global: Arc<TokenBucket>,
    size: u64,
    out_path: &Path,
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
            let req = Request::get(&mirror_url).header("Range", &format!("bytes={}-{}", pos, end));
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
            let ok_status =
                resp.status == 206 || (resp.status == 200 && pos == 0 && end + 1 == size);
            if !ok_status {
                let code = resp.status;
                let _ = resp.read_body(4096);
                conn = resp.into_conn(); // drained error body may still recycle
                retries += 1;
                mirror_stats[mirror_idx].record_fail();
                if retries > MAX_RETRIES {
                    return WorkerExit::Failed(format!("server returned {}", code));
                }
                let next = (mirror_idx + 1) % mirrors_n;
                if next != mirror_idx {
                    mirror_idx = next;
                    conn = None; // conn belongs to the failed mirror
                }
                std::thread::sleep(Duration::from_millis(backoff_ms(retries)));
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
                    let chunk = (take - paced).min(64 * 1024);
                    task_bucket.wait_for(chunk as u64);
                    global.wait_for(chunk as u64);
                    {
                        let f = file.lock().unwrap();
                        if let Err(e) =
                            write_positioned(&f, &buf[paced..paced + chunk], pos + paced as u64)
                        {
                            return WorkerExit::Failed(format!("write: {}", e));
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
            // lease complete: recycle the cleanly drained connection
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
}
