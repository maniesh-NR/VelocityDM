//! Download scheduler (XDM-style): start-at activation, daily start/stop
//! windows, queue concurrency with priorities, shutdown-when-done.
//!
//! The pure decision logic is [`tick`] so it is fully unit-testable with an
//! injected clock; the engine owns a thread that calls it every second.

use serde::{Deserialize, Serialize};

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
pub fn tick(tasks: &[TaskView], rules: &SchedulerRules, now: u64, minute_of_day: u32) -> Vec<Action> {
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
    let active_count = tasks.iter().filter(|t| t.state == TaskViewState::Active).count();
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
    if !tasks.is_empty()
        && tasks.iter().all(|t| t.state == TaskViewState::Done)
    {
        actions.push(Action::QueueDrained);
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tv(id: u64, state: TaskViewState, priority: u8, start_at: Option<u64>) -> TaskView {
        TaskView { id, state, priority, start_at, user_paused: false }
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
}
