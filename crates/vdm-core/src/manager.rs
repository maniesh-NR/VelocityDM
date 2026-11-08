// SPDX-License-Identifier: GPL-3.0-or-later
//! Download manager: task registry, concurrency slots, event fan-out,
//! global rate limiting, and startup resume scanning.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, Semaphore};

use crate::error::{Result, VdmError};
use crate::httpc::HttpClient;
use crate::rate_limiter::TokenBucket;
use crate::range_pool;
use crate::task::{run_task, SharedTaskEnv, TaskHandle};
use crate::types::{Event, Snapshot, TaskSpec, TaskState};

pub struct ManagerStats {
    pub tasks: usize,
    pub running: usize,
    pub queued: usize,
    pub paused: usize,
    pub completed: usize,
    pub failed: usize,
    pub uptime_secs: u64,
}

pub struct Manager {
    env: Arc<SharedTaskEnv>,
    slots: Arc<Semaphore>,
    tasks: Arc<Mutex<BTreeMap<u64, Arc<TaskHandle>>>>,
    next_id: AtomicU64,
    started: Instant,
    download_dir: std::path::PathBuf,
}

impl Manager {
    pub fn new(settings: &crate::settings::Settings) -> Arc<Manager> {
        let settings = settings.clone().sanitized();
        let (events, _) = broadcast::channel::<Event>(1024);
        let env = Arc::new(SharedTaskEnv {
            client: HttpClient::new(settings.user_agent.clone()),
            global_limiter: Arc::new(TokenBucket::new(settings.global_rate_limit_bps)),
            events,
            max_connections_per_host: settings.max_connections_per_host,
            min_segment_bytes: settings.min_segment_bytes,
            default_segments: settings.default_segments,
            extra_headers: Arc::new(settings.extra_headers.clone()),
        });
        Arc::new(Manager {
            env,
            slots: Arc::new(Semaphore::new(settings.max_concurrent_tasks)),
            tasks: Arc::new(Mutex::new(BTreeMap::new())),
            next_id: AtomicU64::new(1),
            started: Instant::now(),
            download_dir: settings.download_dir.clone(),
        })
    }

    pub fn env(&self) -> &Arc<SharedTaskEnv> {
        &self.env
    }

    /// Subscribe to engine events (progress snapshots at the refresh rate,
    /// state transitions, completions, failures).
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.env.events.subscribe()
    }

    pub fn set_global_rate_limit(&self, bps: u64) {
        self.env.global_limiter.set_rate(bps);
    }

    pub fn global_rate_limit(&self) -> u64 {
        self.env.global_limiter.rate()
    }

    pub fn set_refresh_hz(&self, hz: u32) {
        MANAGER_REFRESH_HZ.store(hz.clamp(10, 30) as u64, Ordering::Relaxed);
    }

    pub fn refresh_hz(&self) -> u32 {
        MANAGER_REFRESH_HZ.load(Ordering::Relaxed).clamp(10, 30) as u32
    }

    pub fn download_dir(&self) -> &std::path::Path {
        &self.download_dir
    }

    pub async fn add(&self, mut spec: TaskSpec) -> Result<u64> {
        if spec.url.is_empty() {
            return Err(VdmError::InvalidInput("url is empty".into()));
        }
        url::Url::parse(&spec.url)?;
        for m in &spec.mirrors {
            url::Url::parse(m)?;
        }
        if spec.dir.is_none() {
            spec.dir = Some(self.download_dir.clone());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let handle = TaskHandle::new(id, spec);
        self.tasks.lock().unwrap().insert(id, handle.clone());
        let env = self.env.clone();
        let slots = self.slots.clone();
        let tasks = self.tasks.clone();
        let handle_for_spawn = handle.clone();
        tokio::spawn(async move {
            // Concurrency slot: limits simultaneously RUNNING tasks.
            let _permit = slots.acquire_owned().await;
            if handle_cancelled(&handle_for_spawn) {
                tasks.lock().unwrap().remove(&handle_for_spawn.id);
                return;
            }
            run_task(env, handle_for_spawn).await;
        });
        emit(&self.env, Event::Added(handle.snapshot()));
        Ok(id)
    }

    pub fn get_handle(&self, id: u64) -> Option<Arc<TaskHandle>> {
        self.tasks.lock().unwrap().get(&id).cloned()
    }

    pub fn get(&self, id: u64) -> Option<Snapshot> {
        self.get_handle(id).map(|h| h.snapshot())
    }

    pub fn list(&self) -> Vec<Snapshot> {
        self.tasks
            .lock()
            .unwrap()
            .values()
            .map(|h| h.snapshot())
            .collect()
    }

    /// Requests a pause and waits (bounded) for the state transition so CLI
    /// callers observe a settled state.
    pub async fn pause(&self, id: u64) -> Result<TaskState> {
        let handle = self.get_handle(id).ok_or(VdmError::TaskNotFound(id))?;
        let current = handle.state();
        if current.is_terminal() {
            return Ok(current);
        }
        handle.pause.store(true, Ordering::Relaxed);
        if current == TaskState::Queued {
            // The runner will flip it to Paused at slot acquisition.
            handle.set_state(TaskState::Paused);
        }
        self.wait_for_state(&handle, TaskState::Paused, Duration::from_secs(15)).await;
        Ok(handle.state())
    }

    pub async fn pause_all(&self) -> usize {
        let ids: Vec<u64> = self.tasks.lock().unwrap().keys().copied().collect();
        let mut count = 0;
        for id in ids {
            if self.pause(id).await.is_ok() {
                count += 1;
            }
        }
        count
    }

    pub fn resume(&self, id: u64) -> Result<TaskState> {
        let handle = self.get_handle(id).ok_or(VdmError::TaskNotFound(id))?;
        if handle.state() != TaskState::Paused {
            return Err(VdmError::InvalidInput(format!("task {} is not paused", id)));
        }
        handle.pause.store(false, Ordering::Relaxed);
        handle.set_state(TaskState::Running);
        handle.resume_signal.notify_one();
        Ok(TaskState::Running)
    }

    pub fn resume_all(&self) -> usize {
        let snapshots = self.list();
        let mut count = 0;
        for snap in snapshots {
            if snap.state == TaskState::Paused {
                if self.resume(snap.id).is_ok() {
                    count += 1;
                }
            }
        }
        count
    }

    pub fn remove(&self, id: u64, purge: bool) -> Result<()> {
        let handle = self.get_handle(id).ok_or(VdmError::TaskNotFound(id))?;
        handle.cancel.store(true, Ordering::Relaxed);
        handle.pause.store(false, Ordering::Relaxed);
        // Wake a paused task so it observes the cancel.
        handle.resume_signal.notify_one();
        self.tasks.lock().unwrap().remove(&id);
        emit(&self.env, Event::Removed { id });
        if purge {
            if let Some(path) = handle.final_path.lock().unwrap().clone() {
                let _ = std::fs::remove_file(&path);
            }
            let snapshot = handle.snapshot();
            if let Some(dir) = &handle.spec.dir {
                let filename = snapshot.filename;
                if !filename.is_empty() {
                    let part = dir.join(format!("{}.part", filename));
                    let _ = std::fs::remove_file(&part);
                    range_pool::remove_ctl(&part);
                }
            }
        }
        Ok(())
    }

    pub fn stats(&self) -> ManagerStats {
        let mut stats = ManagerStats {
            tasks: 0,
            running: 0,
            queued: 0,
            paused: 0,
            completed: 0,
            failed: 0,
            uptime_secs: self.started.elapsed().as_secs(),
        };
        for snap in self.list() {
            stats.tasks += 1;
            match snap.state {
                TaskState::Running | TaskState::Probing => stats.running += 1,
                TaskState::Queued => stats.queued += 1,
                TaskState::Paused => stats.paused += 1,
                TaskState::Completed => stats.completed += 1,
                TaskState::Failed => stats.failed += 1,
                TaskState::Removed => {}
            }
        }
        stats
    }

    /// Re-verifies a completed download against its spec checksum.
    pub fn verify(&self, id: u64) -> Result<bool> {
        let handle = self.get_handle(id).ok_or(VdmError::TaskNotFound(id))?;
        let path = handle
            .final_path
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| VdmError::InvalidInput(format!("task {} has no completed file", id)))?;
        let ck = handle
            .spec
            .checksum
            .as_ref()
            .ok_or_else(|| VdmError::InvalidInput(format!("task {} has no expected checksum", id)))?;
        crate::checksum::verify_file(&path, ck.algorithm, &ck.expected)
    }

    /// Scans the download dir for `.part.vdmctl` files and re-adds them.
    /// Returns the number of recovered tasks.
    pub async fn scan_and_resume(&self) -> usize {
        let dir = self.download_dir.clone();
        let mut recovered = 0;
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return 0,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".part.vdmctl") else {
                continue;
            };
            let part_path = dir.join(format!("{}.part", stem));
            let Some(ctl) = range_pool::load_ctl(&part_path) else {
                continue;
            };
            let spec = TaskSpec {
                url: ctl.url.clone(),
                mirrors: ctl.mirrors.clone(),
                filename: Some(ctl.filename.clone()),
                dir: Some(dir.clone()),
                resume: true,
                ..TaskSpec::default()
            };
            if self.add(spec).await.is_ok() {
                recovered += 1;
            }
        }
        recovered
    }

    /// Pauses everything and quiesces; called on daemon shutdown.
    pub async fn shutdown(&self) {
        let ids: Vec<u64> = self.tasks.lock().unwrap().keys().copied().collect();
        for id in ids {
            let _ = self.pause(id).await;
        }
        // Give workers a moment to flush their control files.
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    async fn wait_for_state(&self, handle: &Arc<TaskHandle>, want: TaskState, timeout: Duration) {
        let mut rx = handle.wait_state();
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if *rx.borrow() == want {
                return;
            }
            if handle.state().is_terminal() {
                return;
            }
            tokio::select! {
                _ = rx.changed() => {}
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    }
}

fn emit(env: &SharedTaskEnv, event: Event) {
    let _ = env.events.send(event);
}

fn handle_cancelled(handle: &Arc<TaskHandle>) -> bool {
    handle.cancel.load(Ordering::Relaxed)
}

static MANAGER_REFRESH_HZ: AtomicU64 = AtomicU64::new(12);

impl SharedTaskEnv {
    pub fn effective_refresh_hz(&self) -> u32 {
        MANAGER_REFRESH_HZ.load(Ordering::Relaxed).clamp(10, 30) as u32
    }
}
