//! The engine facade: task registry, add/pause/resume/remove/refresh, torrent
//! and magnet tasks, scheduler thread, global limiter, configuration.

pub mod events;
pub mod gate;
pub mod hls;
pub mod scheduler;
pub mod state;
pub mod task;

use crate::bt::{self, BtOptions, BtSpec, Magnet};
use crate::engine::events::{
    ActiveCounter, EngineShared, EventBus, Event, TaskControl, TaskSnapshot,
    TaskState,
};
use crate::engine::gate::Gate;
use crate::engine::scheduler::{Action, SchedulerRules, TaskViewState, TaskView};
use crate::engine::task::{run_http_task, TaskEnv};

pub use crate::engine::task::HttpOptions;
use crate::token_bucket::TokenBucket;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    #[serde(default = "default_dir")]
    pub download_dir: PathBuf,
    #[serde(default = "default_connections")]
    pub default_connections: usize,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub proxy: Option<crate::proxy::ProxyConfig>,
    /// Global speed limit in bytes/s (0 = unlimited).
    #[serde(default)]
    pub global_limit_bps: u64,
    #[serde(default)]
    pub scheduler: SchedulerRules,
}

fn default_dir() -> PathBuf {
    std::env::temp_dir().join("velocitydm")
}

fn default_connections() -> usize {
    4
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            download_dir: default_dir(),
            default_connections: default_connections(),
            user_agent: String::new(),
            proxy: None,
            global_limit_bps: 0,
            scheduler: SchedulerRules::default(),
        }
    }
}

/// Stored task so pause/resume/refresh can re-run it.
#[derive(Clone)]
enum StoredTask {
    Http { url: String, opts: HttpOptions },
    Torrent { spec: BtSpec, out_dir: Option<String>, priorities: Vec<u8>, limit_bps: u64 },
}

pub struct Engine {
    shared: Arc<EngineShared>,
    cfg: Arc<Mutex<EngineConfig>>,
    gate: Arc<Gate>,
    stored: Arc<Mutex<HashMap<u64, StoredTask>>>,
    next_id: AtomicU64,
    scheduler_stop: Arc<AtomicBool>,
    drained_fired: Arc<AtomicBool>,
}

impl Engine {
    pub fn new(cfg: EngineConfig) -> Arc<Engine> {
        let global_bucket = Arc::new(TokenBucket::new(cfg.global_limit_bps, 128 * 1024));
        let shared = Arc::new(EngineShared {
            snapshots: Arc::new(Mutex::new(std::collections::BTreeMap::new())),
            bus: Arc::new(EventBus::new()),
            global_bucket,
            controls: Mutex::new(HashMap::new()),
            active: ActiveCounter::default(),
        });
        let gate = Arc::new(Gate::new(cfg.scheduler.max_active));
        let engine = Arc::new(Engine {
            shared,
            cfg: Arc::new(Mutex::new(cfg)),
            gate,
            stored: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            scheduler_stop: Arc::new(AtomicBool::new(false)),
            drained_fired: Arc::new(AtomicBool::new(false)),
        });
        engine.spawn_scheduler();
        engine
    }

    fn spawn_scheduler(self: &Arc<Self>) {
        let engine = self.clone();
        let stop = self.scheduler_stop.clone();
        std::thread::spawn(move || {
            let mut drained_handled = false;
            while !stop.load(Ordering::Relaxed) {
                let cfg = engine.cfg.lock().unwrap().clone();
                let views: Vec<TaskView> = {
                    let snaps = engine.shared.snapshots.lock().unwrap();
                    snaps
                        .values()
                        .map(|s| TaskView {
                            id: s.id,
                            state: match s.state {
                                TaskState::Queued | TaskState::Probing => TaskViewState::Queued,
                                TaskState::Downloading | TaskState::Verifying | TaskState::Seeding => {
                                    TaskViewState::Active
                                }
                                TaskState::Paused => TaskViewState::Paused,
                                TaskState::Completed | TaskState::Failed => TaskViewState::Done,
                            },
                            priority: s.priority,
                            start_at: s.start_at,
                            user_paused: s.user_paused,
                        })
                        .collect()
                };
                let now = crate::util::now_ms() / 1000;
                let minute_of_day = (now / 60 % (24 * 60)) as u32; // UTC; see docs
                for action in scheduler::tick(&views, &cfg.scheduler, now, minute_of_day) {
                    match action {
                        Action::Start(id) => {
                            let _ = engine.resume_inner(id);
                        }
                        Action::Pause(id) => {
                            let _ = engine.pause(id);
                        }
                        Action::QueueDrained => {
                            if !drained_handled {
                                drained_handled = true;
                                engine.shared.bus.publish(Event::QueueDrained);
                                if let Some(cmd) = &cfg.scheduler.on_queue_drained {
                                    let _ = std::process::Command::new("sh")
                                        .arg("-c")
                                        .arg(cmd)
                                        .spawn();
                                }
                            }
                        }
                    }
                }
                if !views.iter().any(|v| v.state != TaskViewState::Done) {
                    engine.drained_fired.store(false, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }

    pub fn subscribe(&self) -> std::sync::mpsc::Receiver<Event> {
        self.shared.bus.subscribe()
    }

    pub fn config(&self) -> EngineConfig {
        self.cfg.lock().unwrap().clone()
    }

    pub fn set_config(&self, cfg: EngineConfig) {
        {
            let mut c = self.cfg.lock().unwrap();
            c.download_dir = cfg.download_dir.clone();
            c.default_connections = cfg.default_connections;
            c.user_agent = cfg.user_agent.clone();
            c.proxy = cfg.proxy.clone();
            c.scheduler = cfg.scheduler.clone();
            let rate = cfg.global_limit_bps;
            c.global_limit_bps = rate;
            self.shared.global_bucket.set_rate(rate);
        }
        self.gate.set_max(cfg.scheduler.max_active);
        self.shared.bus.publish(Event::ConfigChanged);
    }

    pub fn set_global_limit(&self, bps: u64) {
        self.cfg.lock().unwrap().global_limit_bps = bps;
        self.shared.global_bucket.set_rate(bps);
        self.shared.bus.publish(Event::ConfigChanged);
    }

    fn env(&self) -> TaskEnv {
        let cfg = self.cfg.lock().unwrap();
        TaskEnv {
            download_dir: cfg.download_dir.clone(),
            default_connections: cfg.default_connections,
            user_agent: if cfg.user_agent.is_empty() {
                "VelocityDM/1.0".to_string()
            } else {
                cfg.user_agent.clone()
            },
            proxy: cfg.proxy.clone(),
            global_bucket: self.shared.global_bucket.clone(),
        }
    }

    fn alloc_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    fn insert_snapshot(&self, id: u64, kind: &str, name: &str, url: &str, opts_start_at: Option<u64>, priority: u8) {
        let cfg = self.cfg.lock().unwrap();
        let snap = TaskSnapshot {
            id,
            kind: kind.to_string(),
            name: name.to_string(),
            url: url.to_string(),
            dir: cfg.download_dir.to_string_lossy().to_string(),
            out_path: String::new(),
            state: TaskState::Queued,
            size: 0,
            done: 0,
            speed: 0,
            eta_secs: 0,
            connections: 0,
            peers: 0,
            seeds: 0,
            priority,
            start_at: opts_start_at,
            error: None,
            checksum_ok: None,
            user_paused: false,
            created_at: crate::util::now_ms(),
        };
        self.shared.snapshots.lock().unwrap().insert(id, snap);
        self.shared.bus.publish(Event::TaskAdded { id });
    }

    /// Add an HTTP/HLS (auto-detected) download.
    pub fn add_download(&self, url: &str, opts: HttpOptions) -> crate::Result<u64> {
        if url_check(url).is_err() {
            return Err(crate::Error::Http(format!("invalid URL: {}", url)));
        }
        let id = self.alloc_id();
        let name = opts
            .filename
            .clone()
            .or_else(|| crate::util::filename_from_url(url))
            .unwrap_or_else(|| "download".to_string());
        self.insert_snapshot(id, "http", &name, url, opts.start_at, opts.priority);
        self.stored.lock().unwrap().insert(
            id,
            StoredTask::Http {
                url: url.to_string(),
                opts: opts.clone(),
            },
        );
        let control = TaskControl::new();
        self.shared.controls.lock().unwrap().insert(id, control.clone());
        self.spawn_http_task(id, url.to_string(), opts, control, true);
        Ok(id)
    }

    /// Expand a batch pattern and add each URL (XDM batch download).
    pub fn add_batch(&self, pattern: &str, opts: HttpOptions) -> crate::Result<Vec<u64>> {
        let urls = crate::batch::expand_pattern(pattern)?;
        let mut ids = Vec::with_capacity(urls.len());
        for u in urls {
            let mut o = opts.clone();
            o.filename = None;
            ids.push(self.add_download(&u, o)?);
        }
        Ok(ids)
    }

    fn spawn_http_task(&self, id: u64, url: String, opts: HttpOptions, control: Arc<TaskControl>, resume: bool) {
        let shared = self.shared.clone();
        let env = self.env();
        let gate = self.gate.clone();
        let start_at = opts.start_at;
        let priority = opts.priority;
        std::thread::spawn(move || {
            if let Some(at) = start_at {
                let now = crate::util::now_ms() / 1000;
                if at > now {
                    // wait until scheduled start (or stop)
                    while (crate::util::now_ms() / 1000) < at {
                        if control.is_stopped() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            }
            let _slot = gate.acquire(id, priority);
            let _ = resume;
            run_http_task(id, shared, env, control, url, opts);
        });
    }

    /// Add a torrent (file/bytes) or magnet download (Motrix/Gopeed feature).
    pub fn add_torrent(
        &self,
        spec: BtSpec,
        out_dir: Option<String>,
        priorities: Vec<u8>,
        limit_bps: u64,
    ) -> crate::Result<u64> {
        let (label, urlish, name) = match &spec {
            BtSpec::TorrentFile(p) => (
                "torrent",
                p.to_string_lossy().to_string(),
                p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "torrent".into()),
            ),
            BtSpec::TorrentBytes(_) => ("torrent", "(in-memory)".to_string(), "torrent".to_string()),
            BtSpec::Magnet(m) => (
                "magnet",
                Magnet::to_uri(&crate::bt::Magnet {
                    info_hash: m.info_hash,
                    name: m.name.clone(),
                    trackers: m.trackers.clone(),
                    length: m.length,
                }),
                m.name.clone().unwrap_or_else(|| "magnet".to_string()),
            ),
        };
        let id = self.alloc_id();
        self.insert_snapshot(id, label, &name, &urlish, None, 0);
        self.stored.lock().unwrap().insert(
            id,
            StoredTask::Torrent {
                spec: spec.clone(),
                out_dir: out_dir.clone(),
                priorities: priorities.clone(),
                limit_bps,
            },
        );
        let control = TaskControl::new();
        self.shared.controls.lock().unwrap().insert(id, control.clone());
        self.spawn_bt_task(id, spec, out_dir, priorities, limit_bps, control);
        Ok(id)
    }

    fn spawn_bt_task(
        &self,
        id: u64,
        spec: BtSpec,
        out_dir: Option<String>,
        priorities: Vec<u8>,
        limit_bps: u64,
        control: Arc<TaskControl>,
    ) {
        let shared = self.shared.clone();
        let env = self.env();
        let gate = self.gate.clone();
        std::thread::spawn(move || {
            let _slot = gate.acquire(id, 0);
            shared.set_state(id, TaskState::Probing);
            let cfg = self_cfg_snapshot(&env);
            let opts = BtOptions {
                out_dir: out_dir
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cfg.download_dir.clone()),
                file_priorities: priorities,
                max_peers: 16,
                sequential: false,
                announce_port: 0,
                connect_timeout: Duration::from_secs(10),
                io_timeout: Duration::from_secs(30),
                stop: Some(control.stop_flag()),
                rate_limit: if limit_bps > 0 {
                    Some(Arc::new(TokenBucket::new(limit_bps, 256 * 1024)))
                } else {
                    None
                },
            };
            let mut last = (std::time::Instant::now(), 0u64);
            let mut speed_ema = 0u64;
            let shared2 = shared.clone();
            let mut on_event = move |evt: bt::BtEvent| match evt {
                bt::BtEvent::Metadata(meta) => {
                    shared2.update(id, |s| {
                        s.name = meta.name.clone();
                        s.size = meta.total_len;
                    });
                }
                bt::BtEvent::Progress { done, total, speed_bps, peers, seeds } => {
                    let now = std::time::Instant::now();
                    let _ = &mut last;
                    let _ = &mut speed_ema;
                    speed_ema = speed_bps;
                    let _ = now;
                    shared2.update(id, |s| {
                        s.done = done;
                        s.size = total;
                        s.speed = speed_bps;
                        s.peers = peers;
                        s.seeds = seeds;
                    });
                    shared2.bus.publish(Event::TaskProgress { id });
                }
                bt::BtEvent::Completed { files, total_len } => {
                    shared2.update(id, |s| {
                        s.out_path = files
                            .first()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if files.len() > 1 {
                            s.dir = files[0]
                                .parent()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();
                        }
                        s.done = total_len;
                        s.size = total_len;
                        s.speed = 0;
                    });
                }
                bt::BtEvent::Failed(msg) => {
                    let _ = msg;
                }
            };
            let result = bt::download_blocking(spec, &opts, &mut on_event);
            match result {
                Ok(done) => {
                    shared.set_state(id, TaskState::Completed);
                    shared.bus.publish(Event::TaskCompleted {
                        id,
                        path: done
                            .files
                            .first()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    });
                }
                Err(e) => {
                    if e.to_string().contains("cancelled") {
                        shared.set_state(id, TaskState::Paused);
                    } else {
                        shared.update(id, |s| {
                            s.state = TaskState::Failed;
                            s.error = Some(e.to_string());
                            s.speed = 0;
                        });
                        shared.bus.publish(Event::TaskFailed {
                            id,
                            error: e.to_string(),
                        });
                    }
                }
            }
        });
    }

    pub fn list(&self) -> Vec<TaskSnapshot> {
        self.shared
            .snapshots
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect()
    }

    pub fn get(&self, id: u64) -> Option<TaskSnapshot> {
        self.shared.snapshots.lock().unwrap().get(&id).cloned()
    }

    pub fn pause(&self, id: u64) -> crate::Result<()> {
        let controls = self.shared.controls.lock().unwrap();
        let control = controls
            .get(&id)
            .ok_or_else(|| crate::Error::Task(format!("no task {}", id)))?;
        control.pause.store(true, Ordering::Relaxed);
        drop(controls);
        self.shared.update(id, |s| s.user_paused = true);
        Ok(())
    }

    pub fn pause_all(&self) {
        let controls = self.shared.controls.lock().unwrap();
        for c in controls.values() {
            c.pause.store(true, Ordering::Relaxed);
        }
    }

    pub fn resume(&self, id: u64) -> crate::Result<()> {
        self.resume_inner(id)
    }

    fn resume_inner(&self, id: u64) -> crate::Result<()> {
        let state = self
            .get(id)
            .map(|s| s.state)
            .ok_or_else(|| crate::Error::Task(format!("no task {}", id)))?;
        if matches!(
            state,
            TaskState::Downloading
                | TaskState::Probing
                | TaskState::Verifying
                | TaskState::Queued
        ) {
            // already running or waiting for a slot; never double-spawn
            return Ok(());
        }
        let stored = self
            .stored
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| crate::Error::Task(format!("no stored task {}", id)))?;
        // fresh control flags
        let control = TaskControl::new();
        self.shared.controls.lock().unwrap().insert(id, control.clone());
        self.shared.update(id, |s| {
            s.state = TaskState::Queued;
            s.error = None;
            s.user_paused = false;
        });
        match stored {
            StoredTask::Http { url, opts } => {
                self.spawn_http_task(id, url, opts, control, true);
            }
            StoredTask::Torrent { spec, out_dir, priorities, limit_bps } => {
                self.spawn_bt_task(id, spec, out_dir, priorities, limit_bps, control);
            }
        }
        Ok(())
    }

    /// Stop and forget a task; optionally delete the `.part` data.
    pub fn remove(&self, id: u64, delete_files: bool) -> crate::Result<()> {
        {
            let controls = self.shared.controls.lock().unwrap();
            if let Some(c) = controls.get(&id) {
                c.stop.store(true, Ordering::Relaxed);
                c.pause.store(true, Ordering::Relaxed);
            }
        }
        if delete_files {
            if let Some(snap) = self.get(id) {
                let out = PathBuf::from(&snap.out_path);
                let _ = std::fs::remove_file(crate::engine::state::ResumeState::part_path(&out));
                let _ = std::fs::remove_file(crate::engine::state::ResumeState::ctl_path(&out));
            }
        }
        self.stored.lock().unwrap().remove(&id);
        self.shared.controls.lock().unwrap().remove(&id);
        self.shared.snapshots.lock().unwrap().remove(&id);
        self.shared.bus.publish(Event::TaskRemoved { id });
        Ok(())
    }

    /// XDM "refresh link": give a paused/failed download a fresh URL and
    /// continue from the existing progress.
    pub fn refresh(&self, id: u64, new_url: &str) -> crate::Result<()> {
        if url_check(new_url).is_err() {
            return Err(crate::Error::Http(format!("invalid URL: {}", new_url)));
        }
        let state = self
            .get(id)
            .map(|s| s.state)
            .ok_or_else(|| crate::Error::Task(format!("no task {}", id)))?;
        if matches!(state, TaskState::Downloading | TaskState::Probing | TaskState::Verifying) {
            return Err(crate::Error::Task("pause the download before refreshing its link".into()));
        }
        let original_name = self.get(id).map(|s| s.name.clone());
        let mut stored = self.stored.lock().unwrap();
        let entry = stored
            .get_mut(&id)
            .ok_or_else(|| crate::Error::Task(format!("no stored task {}", id)))?;
        match entry {
            StoredTask::Http { url, opts } => {
                *url = new_url.to_string();
                // keep the original filename across the refresh (XDM behavior)
                if opts.filename.is_none() {
                    if let Some(name) = &original_name {
                        opts.filename = Some(name.clone());
                    }
                }
            }
            StoredTask::Torrent { .. } => {
                return Err(crate::Error::Task("refresh link applies to HTTP downloads".into()));
            }
        }
        drop(stored);
        self.shared.update(id, |s| s.url = new_url.to_string());
        // update the resume state file's URL if present
        if let Some(snap) = self.get(id) {
            let out = PathBuf::from(&snap.out_path);
            if let Some(mut st) = crate::engine::state::ResumeState::load(&out) {
                st.url = new_url.to_string();
                st.mirrors.retain(|m| m != new_url);
                let _ = st.save(&out);
            }
        }
        self.resume_inner(id)
    }

    pub fn set_priority(&self, id: u64, priority: u8) -> crate::Result<()> {
        self.shared.update(id, |s| s.priority = priority);
        Ok(())
    }

    pub fn shutdown(&self) {
        self.scheduler_stop.store(true, Ordering::Relaxed);
        let controls = self.shared.controls.lock().unwrap();
        for c in controls.values() {
            c.stop.store(true, Ordering::Relaxed);
        }
    }
}

fn self_cfg_snapshot(env: &TaskEnv) -> EngineConfig {
    EngineConfig {
        download_dir: env.download_dir.clone(),
        default_connections: env.default_connections,
        user_agent: env.user_agent.clone(),
        proxy: env.proxy.clone(),
        global_limit_bps: 0,
        scheduler: SchedulerRules::default(),
    }
}

fn url_check(url: &str) -> crate::Result<()> {
    crate::http::Url::parse(url).map(|_| ())
}
