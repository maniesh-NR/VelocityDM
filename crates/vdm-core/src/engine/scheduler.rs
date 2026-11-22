//! Download scheduler (XDM-style): start-at activation, daily start/stop
//! windows, queue concurrency with priorities, shutdown-when-done, and
//! persisted scheduler rules (S11-i B13).
//!
//! The pure decision logic is [`tick`] so it is fully unit-testable with an
//! injected clock; the engine owns a thread that calls it every second.
//! The rules persist to `<download-dir>/.vdm/scheduler.vdmq` (schema ours,
//! atomic temp+rename writes, tolerant loads — never fail startup).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One task as seen by the scheduler.
#[derive(Debug, Clone)]
pub struct TaskView {
    pub id: u64,
    pub state: TaskViewState,
    pub priority: u8,
    pub start_at: Option<u64>,
    /// Paused by the user: the scheduler must not auto-resume it.
    #[allow(dead_code)]
    pub user_paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskViewState {
    Queued,
    Active,
    Paused,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SchedulerRules {
    /// Daily pause window, e.g. ("08:00", "18:00") pauses downloads while you
    /// work (XDM "schedule downloads: start and stop at set times").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_window: Option<(String, String)>,
    /// Run a command when the queue drains (XDM "shut down when done" /
    /// post-download actions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_queue_drained: Option<String>,
    /// Exit the daemon when the queue drains.
    #[serde(default)]
    pub shutdown_when_done: bool,
    /// Maximum simultaneous downloads.
    #[serde(default = "default_max_active")]
    pub max_active: usize,
}

fn default_max_active() -> usize {
    3
}

impl Default for SchedulerRules {
    fn default() -> Self {
        SchedulerRules {
            stop_window: None,
            on_queue_drained: None,
            shutdown_when_done: false,
            max_active: default_max_active(),
        }
    }
}

// ---- S11-i B13: persisted scheduler rules (`.vdmq`) ----

/// On-disk shape of `.vdm/scheduler.vdmq` (schema v1, ours — a distinct
/// suffix the strict Android `.vdmctl` reader never sees). `windows` is a
/// list so the schema is forward-compatible with multi-window scheduling;
/// today's single-queue engine consumes the first entry only.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct SchedulerStateFile {
    version: u32,
    #[serde(default = "default_max_active")]
    max_active: usize,
    windows: Vec<[String; 2]>,
    on_queue_drained: Option<String>,
    exit_when_drained: bool,
}

impl Default for SchedulerStateFile {
    fn default() -> Self {
        SchedulerStateFile {
            version: 0,
            max_active: default_max_active(),
            windows: Vec::new(),
            on_queue_drained: None,
            exit_when_drained: false,
        }
    }
}

impl SchedulerStateFile {
    fn from_rules(rules: &SchedulerRules) -> Self {
        SchedulerStateFile {
            version: 1,
            max_active: rules.max_active,
            windows: rules
                .stop_window
                .as_ref()
                .map(|(a, b)| vec![[a.clone(), b.clone()]])
                .unwrap_or_default(),
            on_queue_drained: rules.on_queue_drained.clone(),
            exit_when_drained: rules.shutdown_when_done,
        }
    }

    fn into_rules(self) -> SchedulerRules {
        SchedulerRules {
            stop_window: self.windows.first().map(|w| (w[0].clone(), w[1].clone())),
            on_queue_drained: self.on_queue_drained,
            shutdown_when_done: self.exit_when_drained,
            max_active: self.max_active,
        }
    }
}

/// The engine's default download directory (the OS temp scratch area).
pub(crate) fn default_engine_dir() -> PathBuf {
    std::env::temp_dir().join("velocitydm")
}

/// Whether the rule store applies for this download directory. The DEFAULT
/// dir is OS-temp scratch: volatile, shared by unrelated engine instances
/// (and wiped by the OS), so the store is skipped there BOTH ways — a fresh
/// install must not resurrect stale temp rules (found live by the
/// e2e-cross CI suite: a stop-window saved by one daemon leaked into a
/// second daemon's startup and auto-paused its tasks inside the window).
/// Configured directories persist + load normally.
pub(crate) fn store_active(dir: &Path) -> bool {
    *dir != default_engine_dir()
}

/// Path of the persisted scheduler rules under a download directory.
pub fn rules_path(dir: &Path) -> PathBuf {
    dir.join(".vdm").join("scheduler.vdmq")
}

/// Persist rules atomically (temp file + rename, same directory).
pub fn save_rules(rules: &SchedulerRules, dir: &Path) -> std::io::Result<()> {
    let path = rules_path(dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&SchedulerStateFile::from_rules(rules))
        .map_err(std::io::Error::other)?;
    let tmp = path.with_extension("vdmq.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

/// Load persisted rules. Tolerant by contract (S11-i B13): unknown keys are
/// ignored, a version mismatch or unreadable file logs one stderr line and
/// yields `None` (defaults) — a rule-store problem NEVER fails startup.
pub fn load_rules(dir: &Path) -> Option<SchedulerRules> {
    let text = std::fs::read_to_string(rules_path(dir)).ok()?;
    let file: SchedulerStateFile = match serde_json::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("scheduler.vdmq: unreadable rules ({}) — using defaults", e);
            return None;
        }
    };
    if file.version != 1 {
        eprintln!(
            "scheduler.vdmq: unknown schema version {} — using defaults",
            file.version
        );
        return None;
    }
    Some(file.into_rules())
}

#[derive(Debug, PartialEq)]
pub enum Action {
    Start(u64),
    Pause(u64),
    QueueDrained,
}

/// Parse "HH:MM" into minutes since midnight.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

/// Whether `now` (epoch secs, local-naive minute-of-day) falls in the window.
/// Windows may wrap midnight ("22:00".."06:00").
pub fn in_stop_window(window: &(String, String), minute_of_day: u32) -> bool {
    let a = parse_hhmm(&window.0).unwrap_or(0);
    let b = parse_hhmm(&window.1).unwrap_or(0);
    if a == b {
        return false;
    }
    if a < b {
        minute_of_day >= a && minute_of_day < b
    } else {
        minute_of_day >= a || minute_of_day < b
    }
}

/// Pure scheduler step. `now` is epoch seconds; `minute_of_day` is the local
/// wall-clock minute (0..=1439) for window checks.
pub fn tick(
    tasks: &[TaskView],
    rules: &SchedulerRules,
    now: u64,
    minute_of_day: u32,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let paused_by_window = rules
        .stop_window
        .as_ref()
        .map(|w| in_stop_window(w, minute_of_day))
        .unwrap_or(false);
    if paused_by_window {
        for t in tasks {
            if t.state == TaskViewState::Active {
                actions.push(Action::Pause(t.id));
            }
        }
        return actions;
    }
    // resume paused tasks (they were paused by the window; user-paused tasks
    // have state Paused too, but resume-on-window-end matches XDM behavior)
    let active_count = tasks
        .iter()
        .filter(|t| t.state == TaskViewState::Active)
        .count();
    let mut slots = rules.max_active.saturating_sub(active_count);
    // candidates: paused (not by the user) first (FIFO), then queued ordered
    // by (priority desc, start_at)
    let mut candidates: Vec<&TaskView> = tasks
        .iter()
        .filter(|t| t.state == TaskViewState::Paused && !t.user_paused)
        .collect();
    candidates.sort_by_key(|t| t.id);
    for t in candidates {
        if slots == 0 {
            break;
        }
        actions.push(Action::Start(t.id));
        slots -= 1;
    }
    let mut queued: Vec<&TaskView> = tasks
        .iter()
        .filter(|t| t.state == TaskViewState::Queued)
        .filter(|t| t.start_at.map(|s| s <= now).unwrap_or(true))
        .collect();
    queued.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
    for t in queued {
        if slots == 0 {
            break;
        }
        actions.push(Action::Start(t.id));
        slots -= 1;
    }
    // queue-drained detection: nothing active/queued/paused and at least one done
    if !tasks.is_empty() && tasks.iter().all(|t| t.state == TaskViewState::Done) {
        actions.push(Action::QueueDrained);
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tv(id: u64, state: TaskViewState, priority: u8, start_at: Option<u64>) -> TaskView {
        TaskView {
            id,
            state,
            priority,
            start_at,
            user_paused: false,
        }
    }

    #[test]
    fn user_paused_not_auto_resumed() {
        let rules = SchedulerRules::default();
        let tasks = vec![TaskView {
            id: 1,
            state: TaskViewState::Paused,
            priority: 0,
            start_at: None,
            user_paused: true,
        }];
        assert!(tick(&tasks, &rules, 0, 0).is_empty());
    }

    #[test]
    fn hhmm_parsing() {
        assert_eq!(parse_hhmm("08:30"), Some(510));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("7:5"), Some(425));
        assert_eq!(parse_hhmm("x"), None);
    }

    #[test]
    fn windows() {
        let w = ("09:00".to_string(), "17:00".to_string());
        assert!(in_stop_window(&w, 9 * 60));
        assert!(in_stop_window(&w, 12 * 60));
        assert!(!in_stop_window(&w, 8 * 60));
        assert!(!in_stop_window(&w, 17 * 60));
        // wrapping window
        let w2 = ("22:00".to_string(), "06:00".to_string());
        assert!(in_stop_window(&w2, 23 * 60));
        assert!(in_stop_window(&w2, 3 * 60));
        assert!(!in_stop_window(&w2, 12 * 60));
        // empty window
        let w3 = ("10:00".to_string(), "10:00".to_string());
        assert!(!in_stop_window(&w3, 600));
    }

    #[test]
    fn activates_by_priority_then_fifo() {
        let rules = SchedulerRules {
            max_active: 2,
            ..Default::default()
        };
        let tasks = vec![
            tv(1, TaskViewState::Queued, 0, None),
            tv(2, TaskViewState::Queued, 5, None),
            tv(3, TaskViewState::Queued, 5, None),
            tv(4, TaskViewState::Queued, 1, None),
        ];
        let actions = tick(&tasks, &rules, 100, 0);
        assert_eq!(
            actions,
            vec![Action::Start(2), Action::Start(3)] // priority desc, then id asc
        );
    }

    #[test]
    fn respects_start_at() {
        let rules = SchedulerRules::default();
        let tasks = vec![tv(1, TaskViewState::Queued, 0, Some(200))];
        assert!(tick(&tasks, &rules, 100, 0).is_empty());
        let actions = tick(&tasks, &rules, 200, 0);
        assert_eq!(actions, vec![Action::Start(1)]);
    }

    #[test]
    fn stop_window_pauses_then_resumes() {
        let rules = SchedulerRules {
            stop_window: Some(("09:00".into(), "17:00".into())),
            ..Default::default()
        };
        let tasks = vec![tv(1, TaskViewState::Active, 0, None)];
        // inside the window: pause
        let actions = tick(&tasks, &rules, 100, 10 * 60);
        assert_eq!(actions, vec![Action::Pause(1)]);
        // engine applied the pause; next tick sees it paused
        let paused = vec![tv(1, TaskViewState::Paused, 0, None)];
        // outside the window: resume paused task
        let actions2 = tick(&paused, &rules, 100, 8 * 60);
        assert_eq!(actions2, vec![Action::Start(1)]);
        // paused + queued both start outside the window
        let mixed = vec![
            tv(1, TaskViewState::Paused, 0, None),
            tv(2, TaskViewState::Queued, 0, None),
        ];
        let actions3 = tick(&mixed, &rules, 100, 8 * 60);
        assert_eq!(actions3, vec![Action::Start(1), Action::Start(2)]);
    }

    #[test]
    fn max_active_limit() {
        let rules = SchedulerRules {
            max_active: 1,
            ..Default::default()
        };
        let tasks = vec![
            tv(1, TaskViewState::Active, 0, None),
            tv(2, TaskViewState::Queued, 0, None),
        ];
        let actions = tick(&tasks, &rules, 0, 0);
        assert!(actions.is_empty());
    }

    #[test]
    fn queue_drained() {
        let rules = SchedulerRules::default();
        let tasks = vec![tv(1, TaskViewState::Done, 0, None)];
        assert_eq!(tick(&tasks, &rules, 0, 0), vec![Action::QueueDrained]);
        let tasks2 = vec![
            tv(1, TaskViewState::Done, 0, None),
            tv(2, TaskViewState::Queued, 0, None),
        ];
        assert!(!tick(&tasks2, &rules, 0, 0).contains(&Action::QueueDrained));
        assert!(tick(&[], &rules, 0, 0).is_empty());
    }

    // ---- S11-i B13: persisted rules (`.vdmq`) ----

    fn sample_rules() -> SchedulerRules {
        SchedulerRules {
            stop_window: Some(("09:00".to_string(), "17:00".to_string())),
            on_queue_drained: Some("echo done".to_string()),
            shutdown_when_done: true,
            max_active: 5,
        }
    }

    #[test]
    fn rules_roundtrip_and_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(&sample_rules(), dir.path()).unwrap();
        let path = rules_path(dir.path());
        assert!(path.is_file(), "missing {:?}", path);
        // atomic rename semantics: no partial/temp file observable
        assert!(
            !dir.path().join(".vdm").exists()
                || std::fs::read_dir(dir.path().join(".vdm"))
                    .unwrap()
                    .filter_map(|e| e.ok())
                    .all(|e| e.file_name() != "scheduler.vdmq.tmp")
        );
        let back = load_rules(dir.path()).unwrap();
        assert_eq!(back, sample_rules());
    }

    #[test]
    fn rules_empty_window_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(&SchedulerRules::default(), dir.path()).unwrap();
        let back = load_rules(dir.path()).unwrap();
        assert_eq!(back, SchedulerRules::default());
        // the on-disk schema carries the owned field names
        let text = std::fs::read_to_string(rules_path(dir.path())).unwrap();
        assert!(text.contains("\"version\""));
        assert!(text.contains("\"exit_when_drained\": false"));
        assert!(text.contains("\"windows\": []"));
    }

    #[test]
    fn rules_unknown_keys_tolerated() {
        let dir = tempfile::tempdir().unwrap();
        let path = rules_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "{\"version\":1,\"max_active\":7,\"windows\":[[\"22:00\",\"06:00\"]],\
             \"on_queue_drained\":null,\"exit_when_drained\":true,\"future_field\":{\"x\":1}}",
        )
        .unwrap();
        let back = load_rules(dir.path()).unwrap();
        assert_eq!(back.max_active, 7);
        assert_eq!(back.stop_window, Some(("22:00".into(), "06:00".into())));
        assert!(back.shutdown_when_done);
    }

    #[test]
    fn store_skips_the_volatile_default_dir() {
        // the OS-temp default is skipped both ways (no save, no load):
        // it is shared scratch across unrelated engine instances
        assert!(!store_active(&default_engine_dir()));
        assert!(store_active(Path::new("/some/configured/dir")));
        let custom = std::env::temp_dir().join("velocitydm-other");
        assert!(store_active(&custom));
    }

    #[test]
    fn rules_version_mismatch_and_garbage_yield_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = rules_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // version 2 from a future schema: defaults, never an error
        std::fs::write(&path, "{\"version\":2,\"max_active\":99}").unwrap();
        assert!(load_rules(dir.path()).is_none());
        // truncated/garbage file: defaults, never an error
        std::fs::write(&path, "{\"version\":1,\"max_active\":").unwrap();
        assert!(load_rules(dir.path()).is_none());
        // missing file: silent None
        let other = tempfile::tempdir().unwrap();
        assert!(load_rules(other.path()).is_none());
    }
}
