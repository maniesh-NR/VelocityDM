// SPDX-License-Identifier: GPL-3.0-or-later
//! Task lifecycle: probe -> (resume?) -> worker fleet (AIMD-tuned) ->
//! checksum -> finalize. Emits coalesced progress snapshots at the UI
//! refresh rate; supports pause/resume/restart via `.part.vdmctl`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{watch, Notify};
use tokio::task::JoinSet;

use crate::conn_tuner::ConnTuner;
use crate::error::{Result, VdmError};
use crate::hls;
use crate::httpc::HttpClient;
use crate::probe;
use crate::range_pool::{self, RangePool};
use crate::rate_limiter::TokenBucket;
use crate::types::{Event, Snapshot, TaskSpec, TaskState};
use crate::worker::{self, WorkerCtx, WorkerStats};

pub struct TaskHandle {
    pub id: u64,
    pub spec: TaskSpec,
    state: RwLock<TaskState>,
    pub pause: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
    pub errors_recent: Arc<AtomicU64>,
    pub downloaded: Arc<AtomicU64>,
    pub total: Mutex<Option<u64>>,
    pub speed_bps: Mutex<f64>,
    pub connections: Arc<AtomicU64>,
    pub error: Mutex<Option<String>>,
    pub final_path: Mutex<Option<std::path::PathBuf>>,
    pub filename: Mutex<String>,
    pub(crate) resume_signal: Notify,
    state_watch: watch::Sender<TaskState>,
}

impl TaskHandle {
    pub(crate) fn new(id: u64, spec: TaskSpec) -> Arc<Self> {
        let filename = spec.filename.clone().unwrap_or_default();
        let (state_watch, _) = watch::channel(TaskState::Queued);
        Arc::new(TaskHandle {
            id,
            spec,
            state: RwLock::new(TaskState::Queued),
            pause: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            errors_recent: Arc::new(AtomicU64::new(0)),
            downloaded: Arc::new(AtomicU64::new(0)),
            total: Mutex::new(None),
            speed_bps: Mutex::new(0.0),
            connections: Arc::new(AtomicU64::new(0)),
            error: Mutex::new(None),
            final_path: Mutex::new(None),
            filename: Mutex::new(filename),
            resume_signal: Notify::new(),
            state_watch,
        })
    }

    pub fn state(&self) -> TaskState {
        *self.state.read().unwrap()
    }

    pub(crate) fn set_state(&self, state: TaskState) {
        *self.state.write().unwrap() = state;
        let _ = self.state_watch.send(state);
    }

    pub fn wait_state(&self) -> watch::Receiver<TaskState> {
        self.state_watch.subscribe()
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.state();
        let total = *self.total.lock().unwrap();
        let downloaded = self.downloaded.load(Ordering::Relaxed);
        let speed = *self.speed_bps.lock().unwrap();
        let eta = match (total, speed) {
            (Some(t), s) if s > 1.0 && t > downloaded => Some(((t - downloaded) as f64 / s) as u64),
            _ => None,
        };
        Snapshot {
            id: self.id,
            url: self.spec.url.clone(),
            filename: self.filename.lock().unwrap().clone(),
            state,
            total,
            downloaded,
            speed_bps: (speed * 10.0).round() / 10.0,
            connections: self.connections.load(Ordering::Relaxed) as u32,
            eta_secs: eta,
            error: self.error.lock().unwrap().clone(),
            path: self.final_path.lock().unwrap().as_ref().map(|p| p.display().to_string()),
        }
    }
}

/// Environment shared by all task supervisors: the HTTP client, the global
/// rate limiter, the event bus, and engine-wide knobs.
pub struct SharedTaskEnv {
    pub client: HttpClient,
    pub global_limiter: Arc<TokenBucket>,
    pub events: tokio::sync::broadcast::Sender<Event>,
    pub max_connections_per_host: u32,
    pub min_segment_bytes: u64,
    pub default_segments: u32,
    pub extra_headers: Arc<Vec<(String, String)>>,
}

fn emit(env: &SharedTaskEnv, event: Event) {
    // Non-blocking broadcast: lagging consumers skip snapshots instead of
    // ever blocking the engine.
    let _ = env.events.send(event);
}

pub async fn run_task(env: Arc<SharedTaskEnv>, handle: Arc<TaskHandle>) {
    match run_inner(env.clone(), &handle).await {
        Ok(()) => {}
        Err(e) => {
            if !matches!(e, VdmError::Cancelled) {
                *handle.error.lock().unwrap() = Some(e.to_string());
                handle.set_state(TaskState::Failed);
                emit(&env, Event::Failed { id: handle.id, error: e.to_string() });
            }
        }
    }
}

async fn run_inner(env: Arc<SharedTaskEnv>, handle: &Arc<TaskHandle>) -> Result<()> {
    // A queued task paused before start never probes.
    if handle.pause.load(Ordering::Relaxed) {
        handle.set_state(TaskState::Paused);
        return Ok(());
    }
    handle.set_state(TaskState::Probing);

    let mut urls: Vec<String> = std::iter::once(handle.spec.url.clone())
        .chain(handle.spec.mirrors.iter().cloned())
        .collect();
    urls.dedup();

    let mut headers: Vec<(String, String)> = env.extra_headers.as_ref().clone();
    if let Some(referer) = &handle.spec.referrer {
        headers.push(("Referer".to_string(), referer.clone()));
    }
    headers.extend(handle.spec.headers.iter().cloned());
    let headers = Arc::new(headers);

    // Probe the first responsive URL; reorder so the working one is first.
    let mut probe_result = None;
    let mut probe_errors = Vec::new();
    for url in urls.clone() {
        match probe::probe(&env.client, &url, headers.as_ref()).await {
            Ok(p) => {
                probe_result = Some((url, p));
                break;
            }
            Err(e) => probe_errors.push(format!("{}: {}", url, e)),
        }
    }
    let (primary_url, probed) = match probe_result {
        Some(x) => x,
        None => {
            return Err(VdmError::Http(format!(
                "probe failed for all mirrors: {}",
                probe_errors.join("; ")
            )))
        }
    };
    let primary_pos = urls.iter().position(|u| *u == primary_url).unwrap_or(0);
    urls.rotate_left(primary_pos);
    let urls = Arc::new(urls);

    // Resolve filename.
    let filename = handle
        .spec
        .filename
        .clone()
        .or_else(|| probed.filename.clone())
        .unwrap_or_else(|| format!("download-{}.bin", handle.id));
    let filename = range_pool::sanitize_filename(&filename)
        .ok_or_else(|| VdmError::InvalidInput("filename resolves to nothing".into()))?;
    *handle.filename.lock().unwrap() = filename.clone();
    *handle.total.lock().unwrap() = probed.total;

    let dir = handle.spec.dir.clone().unwrap_or_else(|| {
        // The manager injects its download dir through spec.dir; fall back to
        // the process working directory when unset (tests).
        std::path::PathBuf::from(".")
    });
    std::fs::create_dir_all(&dir)?;
    let part_path = dir.join(format!("{}.part", filename));
    let final_path = dir.join(&filename);

    // HLS playlists take the sequential pipeline.
    let is_playlist = probed
        .content_type
        .as_deref()
        .map(|t| t.to_ascii_lowercase().contains("mpegurl"))
        .unwrap_or(false)
        || primary_url.to_ascii_lowercase().contains(".m3u8");
    if is_playlist {
        return run_hls_pipeline(env, handle, urls, headers, &final_path).await;
    }

    // Resume or fresh pool.
    let pool = if handle.spec.resume || std::path::Path::new(&part_path).exists() {
        match range_pool::load_ctl(&part_path) {
            Some(ctl)
                if ctl.filename == filename
                    && ctl.total == probed.total
                    && (ctl.etag.is_none() || probed.etag.is_none() || ctl.etag == probed.etag) =>
            {
                RangePool::from_ctl(ctl)
            }
            _ => RangePool::new(probed.total, segments_for(&env, handle, probed.total), 1024 * 1024),
        }
    } else {
        RangePool::new(probed.total, segments_for(&env, handle, probed.total), 1024 * 1024)
    };

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&part_path)?;
    if pool.total.is_some() {
        // Sparse pre-allocation: scatter writes land at absolute offsets.
        let _ = file.set_len(pool.total.unwrap());
    }
    let file = Arc::new(file);

    let pool = Arc::new(pool);
    handle.downloaded.store(pool.downloaded(), Ordering::Relaxed);

    let task_limiter = Arc::new(TokenBucket::new(handle.spec.rate_limit_bps));
    let stats = Arc::new(WorkerStats::default());
    let tuner = ConnTuner::new(1, env.max_connections_per_host.min(handle.spec.segments.max(1)));

    handle.set_state(TaskState::Running);

    let mut join_set: JoinSet<std::result::Result<u64, VdmError>> = JoinSet::new();
    let mut last_downloaded = pool.downloaded();
    let mut last_speed: f64 = 0.0;
    let mut last_persist = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_emit = Instant::now();
    let mut transient_failures: u32 = 0;
    let refresh_interval = Duration::from_secs_f64(1.0 / env.effective_refresh_hz().max(1) as f64);

    let outcome: Result<()> = loop {
        tokio::time::sleep(Duration::from_millis(40)).await;
        if handle.cancel.load(Ordering::Relaxed) {
            break Err(VdmError::Cancelled);
        }

        // Spawn workers toward the tuned target in one burst; extra workers
        // beyond the pending block count exit immediately via the claim
        // path, so over-spawning is cheap and keeps ramp-up tight.
        let active = join_set.len() as u32;
        let target = tuner.target().min(handle.spec.segments.max(1));
        if active < target && pool.has_pending() && !pool.is_done() && !handle.pause.load(Ordering::Relaxed) {
            for _ in active..target {
                let ctx = WorkerCtx {
                    client: env.client.clone(),
                    pool: pool.clone(),
                    file: file.clone(),
                    urls: urls.clone(),
                    headers: headers.clone(),
                    pause: handle.pause.clone(),
                    cancel: handle.cancel.clone(),
                    limiter: task_limiter.clone(),
                    global_limiter: env.global_limiter.clone(),
                    stats: stats.clone(),
                    io_timeout_secs: 30,
                };
                join_set.spawn(worker::run_worker(ctx));
            }
        }
        handle.connections.store(join_set.len() as u64, Ordering::Relaxed);

        // Harvest finished workers; a fatal error or a runaway transient
        // failure budget fails the task instead of hot-looping.
        let mut fatal: Option<VdmError> = None;
        while let Some(res) = join_set.try_join_next() {
            match res {
                Ok(Ok(_)) => {}
                Ok(Err(VdmError::Cancelled)) => {}
                Ok(Err(e)) => {
                    if !crate::error::is_transient(&e) {
                        fatal = Some(e);
                        break;
                    }
                    transient_failures += 1;
                    if transient_failures > 20 {
                        fatal = Some(VdmError::Http(format!("repeated worker failures: {}", e)));
                        break;
                    }
                }
                Err(join_err) => {
                    // Worker task panicked or was cancelled: treat as transient
                    // so the supervisor respawns a replacement worker.
                    transient_failures += 1;
                    if transient_failures > 20 {
                        fatal = Some(VdmError::Http(format!("worker join failure: {}", join_err)));
                        break;
                    }
                }
            }
        }
        if let Some(e) = fatal {
            break Err(e);
        }

        let downloaded = pool.downloaded();
        handle.downloaded.store(downloaded, Ordering::Relaxed);

        // AIMD sample window.
        if last_tick.elapsed() >= Duration::from_secs(1) {
            let dt = last_tick.elapsed().as_secs_f64();
            let speed = (downloaded.saturating_sub(last_downloaded)) as f64 / dt;
            let window_errors = stats.errors_recent.swap(0, Ordering::Relaxed) as u32;
            tuner.sample(speed, &mut last_speed, window_errors);
            *handle.speed_bps.lock().unwrap() = speed;
            last_downloaded = downloaded;
            last_tick = Instant::now();
        }

        // Progress snapshot at the UI refresh rate (10-30 Hz), coalesced:
        // consumers always see the latest state, never a queue of updates.
        if last_emit.elapsed() >= refresh_interval {
            last_emit = Instant::now();
            emit(&env, Event::Progress(handle.snapshot()));
        }

        // Persist resume state every 2 s.
        if last_persist.elapsed() >= Duration::from_secs(2) {
            let ctl = pool.to_ctl(&handle.spec.url, &handle.spec.mirrors_for_ctl(urls.as_ref()), &filename, probed.etag.as_deref());
            let _ = range_pool::save_ctl(&part_path, &ctl);
            last_persist = Instant::now();
        }

        if handle.pause.load(Ordering::Relaxed) {
            // Let workers hit their pause checks and exit.
            while !join_set.is_empty() {
                let _ = join_set.join_next().await;
            }
            let ctl = pool.to_ctl(&handle.spec.url, &handle.spec.mirrors_for_ctl(urls.as_ref()), &filename, probed.etag.as_deref());
            range_pool::save_ctl(&part_path, &ctl)?;
            handle.set_state(TaskState::Paused);
            // Wait for resume or cancel.
            let mut wake: bool = false;
            while !wake {
                tokio::select! {
                    _ = handle.resume_signal.notified() => wake = true,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        if handle.cancel.load(Ordering::Relaxed) {
                            wake = true;
                        }
                    }
                }
            }
            if handle.cancel.load(Ordering::Relaxed) {
                break Err(VdmError::Cancelled);
            }
            handle.set_state(TaskState::Running);
            continue;
        }

        if pool.is_done() && join_set.is_empty() {
            break Ok(());
        }

        // Deadlock guard: no active workers, none spawnable, work remains,
        // and no pause/cancel -> unrecoverable worker failure.
        if join_set.is_empty() && !pool.is_done() && !handle.pause.load(Ordering::Relaxed) {
            let err = handle.error.lock().unwrap().clone();
            if let Some(err) = err {
                break Err(VdmError::Http(err));
            }
            break Err(VdmError::Http("workers stalled with pending blocks".into()));
        }
    };

    match outcome {
        Ok(()) => {
            drop(file);
            // Checksum verification (post-download, streaming).
            if let Some(ck) = &handle.spec.checksum {
                crate::checksum::verify_file(&part_path, ck.algorithm, &ck.expected)?;
            }
            if final_path.exists() {
                std::fs::remove_file(&final_path)?;
            }
            std::fs::rename(&part_path, &final_path)?;
            range_pool::remove_ctl(&part_path);
            let bytes = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
            *handle.final_path.lock().unwrap() = Some(final_path.clone());
            handle.set_state(TaskState::Completed);
            emit(&env, Event::Completed { id: handle.id, path: final_path.display().to_string(), bytes });
            Ok(())
        }
        Err(VdmError::Cancelled) => {
            range_pool::remove_ctl(&part_path);
            let _ = std::fs::remove_file(&part_path);
            Err(VdmError::Cancelled)
        }
        Err(e) => {
            let ctl = pool.to_ctl(&handle.spec.url, &handle.spec.mirrors_for_ctl(urls.as_ref()), &filename, probed.etag.as_deref());
            let _ = range_pool::save_ctl(&part_path, &ctl);
            Err(e)
        }
    }
}

fn segments_for(env: &SharedTaskEnv, handle: &TaskHandle, total: Option<u64>) -> u32 {
    let spec_segments = if handle.spec.segments > 0 {
        handle.spec.segments
    } else {
        env.default_segments
    };
    match total {
        Some(t) if t < env.min_segment_bytes => 1,
        _ => spec_segments,
    }
}

async fn run_hls_pipeline(
    env: Arc<SharedTaskEnv>,
    handle: &Arc<TaskHandle>,
    urls: Arc<Vec<String>>,
    headers: Arc<Vec<(String, String)>>,
    final_path: &std::path::Path,
) -> Result<()> {
    handle.set_state(TaskState::Running);
    let part_path = final_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(format!(
            "{}.part",
            final_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        ));
    let limiter = Arc::new(TokenBucket::new(handle.spec.rate_limit_bps));
    hls::download_playlist(
        &env.client,
        &urls,
        &headers,
        &part_path,
        &handle.pause,
        &handle.cancel,
        &limiter,
        &env.global_limiter,
    )
    .await?;
    if let Some(ck) = &handle.spec.checksum {
        crate::checksum::verify_file(&part_path, ck.algorithm, &ck.expected)?;
    }
    if final_path.exists() {
        std::fs::remove_file(final_path)?;
    }
    std::fs::rename(&part_path, final_path)?;
    let bytes = std::fs::metadata(final_path).map(|m| m.len()).unwrap_or(0);
    *handle.final_path.lock().unwrap() = Some(final_path.to_path_buf());
    *handle.total.lock().unwrap() = Some(bytes);
    handle.downloaded.store(bytes, Ordering::Relaxed);
    handle.set_state(TaskState::Completed);
    emit(&env, Event::Completed { id: handle.id, path: final_path.display().to_string(), bytes });
    Ok(())
}

impl TaskSpec {
    /// ctl persistence wants the full URL list including the working primary.
    pub(crate) fn mirrors_for_ctl(&self, urls: &[String]) -> Vec<String> {
        let mut list: Vec<String> = urls.to_vec();
        if let Some(pos) = list.iter().position(|u| *u == self.url) {
            list.remove(pos);
        }
        list
    }
}
