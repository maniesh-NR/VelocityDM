//! Engine-wide events, snapshots and the download registry.

use crate::token_bucket::TokenBucket;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Task states reported over RPC and rendered in the GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Queued,
    Probing,
    Downloading,
    Paused,
    Verifying,
    Completed,
    Failed,
    Seeding,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Queued => "queued",
            TaskState::Probing => "probing",
            TaskState::Downloading => "downloading",
            TaskState::Paused => "paused",
            TaskState::Verifying => "verifying",
            TaskState::Completed => "completed",
            TaskState::Failed => "failed",
            TaskState::Seeding => "seeding",
        }
    }
}

/// A public, serializable view of one download.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: u64,
    /// "http" | "hls" | "torrent" | "magnet"
    pub kind: String,
    pub name: String,
    pub url: String,
    pub dir: String,
    pub out_path: String,
    pub state: TaskState,
    pub size: u64,
    pub done: u64,
    /// Bytes per second (EMA).
    pub speed: u64,
    pub eta_secs: u64,
    pub connections: u32,
    pub peers: u32,
    pub seeds: u32,
    pub priority: u8,
    /// Epoch seconds when a queued task should start (scheduler).
    pub start_at: Option<u64>,
    pub error: Option<String>,
    pub checksum_ok: Option<bool>,
    /// Paused by the user (not by the scheduler stop-window).
    #[serde(default)]
    pub user_paused: bool,
    pub created_at: u64,
}

impl TaskSnapshot {
    pub fn compute_eta(&mut self) {
        if self.speed > 0 && self.size > self.done {
            self.eta_secs = (self.size - self.done) / self.speed;
        } else {
            self.eta_secs = 0;
        }
    }
}

/// Coarse engine events (fine-grained progress comes from snapshot polling).
#[derive(Debug, Clone)]
pub enum Event {
    TaskAdded { id: u64 },
    TaskStateChanged { id: u64, state: TaskState },
    TaskProgress { id: u64 },
    TaskCompleted { id: u64, path: String },
    TaskFailed { id: u64, error: String },
    TaskRemoved { id: u64 },
    /// Queue became empty after at least one completion (for
    /// scheduler/shutdown-when-done handling).
    QueueDrained,
    ConfigChanged,
}

pub type EventSender = Arc<EventBus>;

pub struct EventBus {
    senders: Mutex<Vec<SyncSender<Event>>>,
}

impl EventBus {
    pub fn new() -> Self {
        EventBus {
            senders: Mutex::new(Vec::new()),
        }
    }

    pub fn subscribe(&self) -> Receiver<Event> {
        let (tx, rx) = sync_channel(1024);
        self.senders.lock().unwrap().push(tx);
        rx
    }

    pub fn publish(&self, evt: Event) {
        let mut senders = self.senders.lock().unwrap();
        senders.retain(|tx| tx.send(evt.clone()).is_ok());
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared registry of live snapshots; task threads update at ~10 Hz,
/// pollers (RPC SSE, GUI) read at 10-30 Hz.
pub type SnapshotStore = Arc<Mutex<BTreeMap<u64, TaskSnapshot>>>;

/// Live task counter for the scheduler.
#[derive(Default)]
pub struct ActiveCounter {
    pub active: AtomicU64,
}

impl ActiveCounter {
    pub fn inc(&self) {
        self.active.fetch_add(1, Ordering::SeqCst);
    }
    pub fn dec(&self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
    pub fn get(&self) -> u64 {
        self.active.load(Ordering::SeqCst)
    }
}

/// Per-task control flags shared between the engine and worker threads.
pub struct TaskControl {
    pub stop: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}

impl TaskControl {
    pub fn new() -> Arc<TaskControl> {
        Arc::new(TaskControl {
            stop: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub fn is_paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }
    /// Shared stop flag (e.g. for the BT engine options).
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
}

impl Default for TaskControl {
    fn default() -> Self {
        TaskControl {
            stop: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Global shared state an engine task needs.
pub struct EngineShared {
    pub snapshots: SnapshotStore,
    pub bus: EventSender,
    /// Global rate limiter shared by all tasks (XDM global speed limiter).
    pub global_bucket: Arc<TokenBucket>,
    /// Task handles by id.
    pub controls: Mutex<std::collections::HashMap<u64, Arc<TaskControl>>>,
    pub active: ActiveCounter,
}

impl EngineShared {
    pub fn update<F: FnOnce(&mut TaskSnapshot)>(&self, id: u64, f: F) {
        if let Some(snap) = self.snapshots.lock().unwrap().get_mut(&id) {
            f(snap);
            snap.compute_eta();
        }
    }

    pub fn set_state(&self, id: u64, state: TaskState) {
        self.update(id, |s| s.state = state);
        self.bus.publish(Event::TaskStateChanged { id, state });
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_fanout_and_cleanup() {
        let bus = EventBus::new();
        let rx1 = bus.subscribe();
        let rx2 = bus.subscribe();
        bus.publish(Event::TaskAdded { id: 1 });
        assert!(matches!(rx1.try_recv(), Ok(Event::TaskAdded { id: 1 })));
        assert!(matches!(rx2.try_recv(), Ok(Event::TaskAdded { id: 1 })));
        drop(rx1);
        bus.publish(Event::TaskRemoved { id: 1 });
        assert!(rx1_disconnected(&bus));
        assert!(matches!(rx2.try_recv(), Ok(Event::TaskRemoved { id: 1 })));
    }

    fn rx1_disconnected(bus: &EventBus) -> bool {
        bus.senders.lock().unwrap().len() == 1
    }

    #[test]
    fn snapshot_eta() {
        let mut s = TaskSnapshot {
            id: 1,
            kind: "http".into(),
            name: "x".into(),
            url: "http://x/".into(),
            dir: "/tmp".into(),
            out_path: "/tmp/x".into(),
            state: TaskState::Downloading,
            size: 1000,
            done: 100,
            speed: 100,
            eta_secs: 0,
            connections: 4,
            peers: 0,
            seeds: 0,
            priority: 0,
            start_at: None,
            error: None,
            checksum_ok: None,
            user_paused: false,
            created_at: 0,
        };
        s.compute_eta();
        assert_eq!(s.eta_secs, 9);
        s.speed = 0;
        s.compute_eta();
        assert_eq!(s.eta_secs, 0);
    }

    #[test]
    fn active_counter() {
        let c = ActiveCounter::default();
        c.inc();
        c.inc();
        assert_eq!(c.get(), 2);
        c.dec();
        assert_eq!(c.get(), 1);
    }
}
