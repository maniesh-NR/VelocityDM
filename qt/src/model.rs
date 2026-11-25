//! Pure task-list logic for the Qt frontend — NO Qt types in this module
//! (headless-testable without a QApplication or QQmlEngine; the bridge in
//! `app_model.rs` only moves these values across).

#![deny(unsafe_code)]

use vdm_core::engine::events::{TaskSnapshot, TaskState};
use vdm_core::engine::EngineConfig;
use vdm_core::util::{format_bytes, parse_bytes};

/// Control characters (C0 + DEL + C1) map to spaces; the result is
/// trimmed. Applied to EVERY daemon-derived string before rendering (the
/// gtk3 `display_text` parity — QML `Text.PlainText` then renders it
/// literally).
pub fn display_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let n = c as u32;
        if n < 0x20 || n == 0x7F || (0x80..=0x9F).contains(&n) {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// The state chip mapping (gtk3 parity: label + semantic kind).
pub fn state_chip(state: TaskState) -> (&'static str, &'static str) {
    match state {
        TaskState::Completed => ("done", "done"),
        TaskState::Failed => ("failed", "failed"),
        TaskState::Paused | TaskState::Queued => ("queued", "queued"),
        TaskState::Downloading => ("active", "active"),
        TaskState::Verifying => ("verify", "verify"),
        TaskState::Probing => ("probe", "probe"),
        TaskState::Seeding => ("seed", "seed"),
    }
}

pub fn can_pause(state: TaskState) -> bool {
    matches!(
        state,
        TaskState::Downloading | TaskState::Probing | TaskState::Queued
    )
}

pub fn can_resume(state: TaskState) -> bool {
    matches!(state, TaskState::Paused | TaskState::Failed)
}

/// The per-row progress fraction, clamped to 0.0..=1.0 (size 0 => 0).
pub fn progress_fraction(done: u64, size: u64) -> f64 {
    if size == 0 {
        0.0
    } else {
        (done as f64 / size as f64).clamp(0.0, 1.0)
    }
}

/// The one-line stats string: done/total + speed (+ ETA while
/// downloading; BT peers/seeds for torrent tasks).
pub fn stats_line(s: &TaskSnapshot) -> String {
    let mut out = format!(
        "{} / {}",
        format_bytes(s.done),
        if s.size > 0 {
            format_bytes(s.size)
        } else {
            "?".into()
        }
    );
    if s.speed > 0 {
        out.push_str(&format!(" - {}/s", format_bytes(s.speed)));
    }
    if s.state == TaskState::Downloading && s.eta_secs > 0 {
        out.push_str(&format!(" - eta {}s", s.eta_secs));
    }
    if s.kind == "torrent" || s.kind == "magnet" {
        out.push_str(&format!(" - {}p/{}s", s.peers, s.seeds));
    }
    out
}

/// The proxy line for the settings panel ("" when unset).
pub fn proxy_display(cfg: &EngineConfig) -> String {
    match &cfg.proxy {
        None => String::new(),
        Some(vdm_core::proxy::ProxyConfig::Http {
            host,
            port,
            user: _,
            pass: _,
        }) => format!("http://{host}:{port}"),
        Some(vdm_core::proxy::ProxyConfig::Socks5 {
            host,
            port,
            user: _,
            pass: _,
        }) => format!("socks5://{host}:{port}"),
    }
}

/// Parse a proxy field: `http://host:port` or `socks5://host:port`;
/// empty or "none" clears the proxy. Returns None on an unparseable
/// value (tolerant: the caller keeps the old proxy and surfaces an
/// error).
pub fn parse_proxy_field(text: &str) -> Option<Option<vdm_core::proxy::ProxyConfig>> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    for (scheme, socks) in [("http://", false), ("socks5://", true)] {
        if let Some(rest) = t.strip_prefix(scheme) {
            let (host, port) = rest.rsplit_once(':')?;
            let port: u16 = port.parse().ok()?;
            if host.is_empty() {
                return None;
            }
            return Some(Some(if socks {
                vdm_core::proxy::ProxyConfig::Socks5 {
                    host: host.to_string(),
                    port,
                    user: None,
                    pass: None,
                }
            } else {
                vdm_core::proxy::ProxyConfig::Http {
                    host: host.to_string(),
                    port,
                    user: None,
                    pass: None,
                }
            }));
        }
    }
    None
}

/// The settings-panel strings captured from the input fields.
#[derive(Debug, Clone, Default)]
pub struct SettingsFields {
    pub download_dir: String,
    pub connections: String,
    pub max_active: String,
    pub global_limit: String,
    pub proxy: String,
    pub stop_from: String,
    pub stop_to: String,
    pub shutdown_when_done: bool,
}

/// Apply the captured settings fields to a config: starts from the LIVE
/// config so unexposed fields (`user_agent`, `tls_min`, scheduler rules
/// beyond the exposed ones) survive; clamps connections to 1..=32 and
/// max_active to 1..=16; only sets the stop window when BOTH ends parse
/// as HH:MM; clears the proxy on empty/"none"; returns a user-facing
/// error for an unparseable proxy (the config is then left unchanged in
/// that field).
pub fn apply_settings(cfg: &mut EngineConfig, f: &SettingsFields) -> Result<(), String> {
    let dir = f.download_dir.trim().to_string();
    if !dir.is_empty() {
        cfg.download_dir = std::path::PathBuf::from(dir);
    }
    if let Ok(v) = f.connections.trim().parse::<u64>() {
        cfg.default_connections = v.clamp(1, 32) as usize;
    }
    if let Ok(v) = f.max_active.trim().parse::<u64>() {
        cfg.scheduler.max_active = v.clamp(1, 16) as usize;
    }
    if let Some(v) = parse_bytes(f.global_limit.trim()) {
        cfg.global_limit_bps = v;
    }
    match parse_proxy_field(&f.proxy) {
        Some(Some(p)) => cfg.proxy = Some(p),
        Some(None) => cfg.proxy = None,
        None => {
            return Err(format!(
                "cannot parse proxy '{}' (use http://host:port or socks5://host:port)",
                display_text(&f.proxy)
            ))
        }
    }
    let from = f.stop_from.trim();
    let to = f.stop_to.trim();
    if from.is_empty() && to.is_empty() {
        cfg.scheduler.stop_window = None;
    } else if is_hhmm(from) && is_hhmm(to) {
        cfg.scheduler.stop_window = Some((from.to_string(), to.to_string()));
    }
    cfg.scheduler.shutdown_when_done = f.shutdown_when_done;
    Ok(())
}

/// HH:MM validation (both ends must parse or the window stays unset).
pub fn is_hhmm(s: &str) -> bool {
    let Some((h, m)) = s.split_once(':') else {
        return false;
    };
    let Ok(h) = h.parse::<u32>() else {
        return false;
    };
    let Ok(m) = m.parse::<u32>() else {
        return false;
    };
    h < 24 && m < 60
}

/// One row's pure rendering data (everything `data()` serves).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowData {
    pub id: u64,
    pub name: String,
    pub state_label: &'static str,
    pub state_kind: &'static str,
    pub progress_permille: i32,
    pub stats: String,
    pub error_text: String,
    pub notice_text: String,
    pub can_pause: bool,
    pub can_resume: bool,
}

/// Build the row list from engine snapshots (sanitized + formatted).
pub fn rows_from_snapshots(snaps: &[TaskSnapshot]) -> Vec<RowData> {
    snaps
        .iter()
        .map(|s| {
            let (label, kind) = state_chip(s.state);
            RowData {
                id: s.id,
                name: display_text(&s.name),
                state_label: label,
                state_kind: kind,
                progress_permille: (progress_fraction(s.done, s.size) * 1000.0) as i32,
                stats: display_text(&stats_line(s)),
                error_text: s.error.as_deref().map(display_text).unwrap_or_default(),
                notice_text: s.notice.as_deref().map(display_text).unwrap_or_default(),
                can_pause: can_pause(s.state),
                can_resume: can_resume(s.state),
            }
        })
        .collect()
}

/// Indices whose rows changed (or would be new) — drives the per-index
/// `dataChanged` emissions on the in-place update path.
pub fn changed_indices(old: &[RowData], new: &[RowData]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, n) in new.iter().enumerate() {
        if old.get(i) != Some(n) {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(id: u64, state: TaskState, done: u64, size: u64) -> TaskSnapshot {
        TaskSnapshot {
            id,
            kind: "http".into(),
            name: format!("file {id}.bin"),
            url: format!("https://x.test/{id}"),
            dir: "/tmp".into(),
            out_path: format!("/tmp/file {id}.bin"),
            state,
            size,
            done,
            speed: 1024,
            eta_secs: 7,
            connections: 4,
            peers: 0,
            seeds: 0,
            priority: 0,
            start_at: None,
            user_paused: false,
            created_at: 0,
            error: None,
            notice: None,
            checksum_ok: None,
        }
    }

    #[test]
    fn display_text_is_literal_and_control_free() {
        // markdown/HTML survive literally (PlainText rendering contract)
        assert_eq!(display_text("**bold** <b>x</b>"), "**bold** <b>x</b>");
        assert_eq!(display_text("a\u{1}b\u{7f}c\u{85}d"), "a b c d");
        assert_eq!(display_text("  padded  "), "padded");
    }

    #[test]
    fn chip_mapping_is_exhaustive() {
        let states = [
            TaskState::Queued,
            TaskState::Probing,
            TaskState::Downloading,
            TaskState::Paused,
            TaskState::Verifying,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Seeding,
        ];
        // 8 states, 7 distinct chips: Paused and Queued deliberately share
        // the "queued" chip (gtk3 parity); every state must map somewhere
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for s in states {
            let (label, kind) = state_chip(s);
            assert!(
                !label.is_empty() && !kind.is_empty(),
                "empty chip {label:?}/{kind:?}"
            );
            if !seen.contains(&(label, kind)) {
                seen.push((label, kind));
            }
        }
        assert_eq!(seen.len(), 7, "Paused/Queued share a chip by design");
    }

    #[test]
    fn pause_resume_truth_table() {
        assert!(can_pause(TaskState::Downloading));
        assert!(can_pause(TaskState::Probing));
        assert!(can_pause(TaskState::Queued));
        assert!(!can_pause(TaskState::Paused));
        assert!(can_resume(TaskState::Paused));
        assert!(can_resume(TaskState::Failed));
        assert!(!can_resume(TaskState::Downloading));
    }

    #[test]
    fn progress_clamps() {
        assert_eq!(progress_fraction(5, 10), 0.5);
        assert_eq!(progress_fraction(11, 10), 1.0);
        assert_eq!(progress_fraction(0, 0), 0.0);
    }

    #[test]
    fn rows_carry_sanitized_fields() {
        let mut s = snap(1, TaskState::Downloading, 10, 100);
        s.name = "evil\u{1}.bin".into();
        s.error = Some("boom\u{2}".into());
        s.notice = Some("note <b>".into());
        s.kind = "torrent".into();
        s.peers = 3;
        s.seeds = 2;
        let rows = rows_from_snapshots(&[s]);
        assert_eq!(rows[0].name, "evil .bin");
        assert_eq!(rows[0].error_text, "boom"); // trailing control-space is trimmed
        assert_eq!(rows[0].notice_text, "note <b>");
        assert_eq!(rows[0].progress_permille, 100);
        assert!(rows[0].stats.contains("3p/2s"), "{}", rows[0].stats);
        assert!(rows[0].can_pause);
    }

    #[test]
    fn diff_detects_changed_and_new_rows() {
        let a = rows_from_snapshots(&[snap(1, TaskState::Queued, 0, 10)]);
        let b = rows_from_snapshots(&[
            snap(1, TaskState::Downloading, 5, 10),
            snap(2, TaskState::Queued, 0, 5),
        ]);
        assert_eq!(changed_indices(&a, &b), vec![0, 1]);
        assert!(changed_indices(&a, &a).is_empty());
    }

    #[test]
    fn settings_apply_clamps_and_parses() {
        let mut cfg = EngineConfig {
            user_agent: "keep me".into(),
            ..Default::default()
        };
        let f = SettingsFields {
            download_dir: "/tmp/dl".into(),
            connections: "99".into(),
            max_active: "0".into(),
            global_limit: "2M".into(),
            proxy: "socks5://127.0.0.1:1080".into(),
            stop_from: "09:30".into(),
            stop_to: "17:45".into(),
            shutdown_when_done: true,
        };
        apply_settings(&mut cfg, &f).unwrap();
        assert_eq!(cfg.default_connections, 32); // clamped
        assert_eq!(cfg.scheduler.max_active, 1); // clamped
        assert_eq!(cfg.global_limit_bps, 2_097_152); // 1024-based
        assert_eq!(cfg.download_dir, std::path::PathBuf::from("/tmp/dl"));
        assert!(matches!(
            &cfg.proxy,
            Some(vdm_core::proxy::ProxyConfig::Socks5 { port, .. }) if *port == 1080
        ));
        assert_eq!(
            cfg.scheduler
                .stop_window
                .as_ref()
                .map(|(a, b)| (a.as_str(), b.as_str())),
            Some(("09:30", "17:45"))
        );
        assert!(cfg.scheduler.shutdown_when_done);
        assert_eq!(cfg.user_agent, "keep me"); // unexposed field survives
                                               // "none" clears the proxy
        let f2 = SettingsFields {
            proxy: "none".into(),
            ..f.clone()
        };
        apply_settings(&mut cfg, &f2).unwrap();
        assert!(cfg.proxy.is_none());
        // a bad proxy is refused with an error, tolerantly
        let f3 = SettingsFields {
            proxy: "ftp://x".into(),
            ..f2
        };
        assert!(apply_settings(&mut cfg, &f3).is_err());
        // stop window only set when BOTH ends parse
        let mut cfg2: EngineConfig = Default::default();
        let f4 = SettingsFields {
            stop_from: "9:30".into(),
            stop_to: "".into(),
            ..Default::default()
        };
        apply_settings(&mut cfg2, &f4).unwrap();
        assert!(cfg2.scheduler.stop_window.is_none());
        // one-sided window is not set (both ends must parse)
        let f5 = SettingsFields {
            stop_from: "09:30".into(),
            stop_to: "".into(),
            ..Default::default()
        };
        apply_settings(&mut cfg2, &f5).unwrap();
        assert!(cfg2.scheduler.stop_window.is_none());
    }

    #[test]
    fn hhmm_validation() {
        assert!(is_hhmm("00:00"));
        assert!(is_hhmm("23:59"));
        assert!(!is_hhmm("24:00"));
        assert!(!is_hhmm("12:60"));
        assert!(!is_hhmm("123"));
        assert!(!is_hhmm("ab:cd"));
    }

    #[test]
    fn proxy_display_both_variants() {
        let mut cfg: EngineConfig = Default::default();
        assert_eq!(proxy_display(&cfg), "");
        cfg.proxy = Some(vdm_core::proxy::ProxyConfig::Http {
            host: "127.0.0.1".into(),
            port: 8080,
            user: None,
            pass: None,
        });
        assert_eq!(proxy_display(&cfg), "http://127.0.0.1:8080");
        cfg.proxy = Some(vdm_core::proxy::ProxyConfig::Socks5 {
            host: "10.0.0.9".into(),
            port: 1080,
            user: None,
            pass: None,
        });
        assert_eq!(proxy_display(&cfg), "socks5://10.0.0.9:1080");
    }
}
