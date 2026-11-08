// SPDX-License-Identifier: GPL-3.0-or-later
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::checksum::Algorithm;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    Probing,
    Running,
    Paused,
    Completed,
    Failed,
    Removed,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Queued => "queued",
            TaskState::Probing => "probing",
            TaskState::Running => "running",
            TaskState::Paused => "paused",
            TaskState::Completed => "completed",
            TaskState::Failed => "failed",
            TaskState::Removed => "removed",
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskState::Completed | TaskState::Failed | TaskState::Removed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumSpec {
    pub algorithm: Algorithm,
    pub expected: String,
}

/// Everything needed to start a download. Serializable so the RPC layer can
/// build it straight from client JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub url: String,
    #[serde(default)]
    pub mirrors: Vec<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub dir: Option<PathBuf>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default = "default_segments")]
    pub segments: u32,
    /// Per-task rate limit in bytes/sec; 0 = unlimited.
    #[serde(default)]
    pub rate_limit_bps: u64,
    #[serde(default)]
    pub checksum: Option<ChecksumSpec>,
    #[serde(default)]
    pub referrer: Option<String>,
    /// Continue from an existing `.part.vdmctl` when present.
    #[serde(default)]
    pub resume: bool,
}

fn default_segments() -> u32 {
    8
}

impl Default for TaskSpec {
    fn default() -> Self {
        TaskSpec {
            url: String::new(),
            mirrors: Vec::new(),
            filename: None,
            dir: None,
            headers: Vec::new(),
            segments: default_segments(),
            rate_limit_bps: 0,
            checksum: None,
            referrer: None,
            resume: false,
        }
    }
}

/// Immutable progress snapshot, produced at the UI refresh rate (10-30 Hz)
/// and never faster; consumers render these directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: u64,
    pub url: String,
    pub filename: String,
    pub state: TaskState,
    pub total: Option<u64>,
    pub downloaded: u64,
    pub speed_bps: f64,
    pub connections: u32,
    pub eta_secs: Option<u64>,
    pub error: Option<String>,
    pub path: Option<String>,
}

/// Engine events. Serialized with tag "event" / content "params" so the RPC
/// layer forwards them verbatim as `{"event": ..., "params": ...}` frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "params", rename_all = "snake_case")]
pub enum Event {
    Added(Snapshot),
    Progress(Snapshot),
    StateChanged { id: u64, state: TaskState, error: Option<String> },
    Completed { id: u64, path: String, bytes: u64 },
    Failed { id: u64, error: String },
    Removed { id: u64 },
}

/// Percent-decodes a URL path segment, mirroring the extension's decoder.
pub fn percent_decode(input: &str) -> String {
    percent_encoding::percent_decode_str(input).decode_utf8_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("my%20video.mp4"), "my video.mp4");
        assert_eq!(percent_decode("plain.zip"), "plain.zip");
        assert_eq!(percent_decode("caf%C3%A9.pdf"), "caf\u{e9}.pdf");
    }

    #[test]
    fn task_spec_deserializes_from_sparse_json() {
        let spec: TaskSpec = serde_json::from_str(r#"{"url":"https://example.com/a.zip"}"#).unwrap();
        assert_eq!(spec.segments, 8);
        assert!(spec.mirrors.is_empty());
        assert!(!spec.resume);
    }

    #[test]
    fn event_serializes_with_tag_shape() {
        let snap = Snapshot {
            id: 1,
            url: "u".into(),
            filename: "f".into(),
            state: TaskState::Running,
            total: Some(10),
            downloaded: 5,
            speed_bps: 1.0,
            connections: 2,
            eta_secs: Some(5),
            error: None,
            path: None,
        };
        let v = serde_json::to_value(&Event::Added(snap)).unwrap();
        assert_eq!(v["event"], "added");
        assert_eq!(v["params"]["id"], 1);
    }
}
