//! Material You rendering for GTK3 — the `vdm-md3` token engine drives
//! a `GtkCssProvider` stylesheet, and the task-state chip mapping is
//! shared with the other frontends.
//!
//! Gate-0012 safety policies (spec `docs/SPECS/S12-LIN2-GTK3.md`
//! rules 4-6): the stylesheet interpolates ONLY hex strings derived
//! from `vdm-md3` `u32` tokens — never runtime strings — and every
//! attacker-controlled string renders through `GtkLabel::set_text`
//! (this crate never calls `set_markup`, so Pango markup from a
//! server-controlled task name/error/notice can neither spoof the UI
//! nor blank a label). GTK3 CSS has no `var()` custom properties, so
//! theme changes reload the provider's data in place. Token subset:
//! the same 19 of 27 Scheme roles the Slint frontend consumes.

use std::path::PathBuf;

use vdm_core::engine::events::{TaskSnapshot, TaskState};
use vdm_core::engine::EngineConfig;
use vdm_core::proxy::ProxyConfig;
use vdm_core::util::parse_bytes;
use vdm_md3::Scheme;

fn hex_css(value: u32) -> String {
    format!("#{:06x}", value & 0xFF_FFFF)
}

/// Text prepared for `GtkLabel::set_text`. `set_text` displays the
/// string verbatim — Pango markup is never parsed anywhere in this
/// crate — so the only normalization applied here is control-character
/// replacement (a newline smuggled inside a task name or error would
/// otherwise break the single-line row layout). Every other byte,
/// including `<span ...>` payloads, passes through literally.
pub fn display_text(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// (chip text, CSS class) — the same mapping as the Slint frontend's
/// `state_chip_by`, with the color riding a stylesheet class instead of
/// a runtime color value.
pub fn state_chip(state: TaskState) -> (&'static str, &'static str) {
    match state {
        TaskState::Completed => ("done", "chip-done"),
        TaskState::Failed => ("failed", "chip-failed"),
        TaskState::Paused | TaskState::Queued => ("queued", "chip-queued"),
        TaskState::Downloading => ("active", "chip-active"),
        TaskState::Verifying => ("verify", "chip-verify"),
        TaskState::Probing => ("probe", "chip-probe"),
        TaskState::Seeding => ("seed", "chip-seed"),
    }
}

/// Same can-pause/can-resume mapping as the Slint row model.
pub fn can_pause(state: TaskState) -> bool {
    matches!(
        state,
        TaskState::Downloading | TaskState::Probing | TaskState::Queued
    )
}

pub fn can_resume(state: TaskState) -> bool {
    matches!(state, TaskState::Paused | TaskState::Failed)
}

pub fn progress_fraction(done: u64, size: u64) -> f64 {
    if size > 0 {
        (done as f64 / size as f64).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub fn proxy_display(p: &ProxyConfig) -> String {
    match p {
        ProxyConfig::Http { host, port, .. } => format!("http://{host}:{port}"),
        ProxyConfig::Socks5 { host, port, .. } => format!("socks5://{host}:{port}"),
    }
}

/// Stats line — the same shape as the Slint row stats.
pub fn stats_line(s: &TaskSnapshot) -> String {
    let eta = if s.eta_secs > 0 && s.state == TaskState::Downloading {
        format!("{}s", s.eta_secs)
    } else {
        "-".to_string()
    };
    let extra = if s.peers > 0 {
        format!(" {}p/{}s", s.peers, s.seeds)
    } else {
        String::new()
    };
    let speed = if s.speed > 0 {
        format!("{}/s", vdm_core::util::format_bytes(s.speed))
    } else {
        "-".to_string()
    };
    format!(
        "{} / {}  {}  {}{}",
        vdm_core::util::format_bytes(s.done),
        if s.size > 0 {
            vdm_core::util::format_bytes(s.size)
        } else {
            "?".to_string()
        },
        speed,
        eta,
        extra
    )
}

/// The GTK3 stylesheet from MD3 tokens. Every interpolation is a hex
/// string produced by `hex_css` from a `Scheme` field; the selectors
/// are static literals — no runtime string ever reaches the CSS (rule
/// 5). Token subset = the same 19 of 27 roles Slint consumes:
/// primary/on_primary, primary_container/on_primary_container,
/// secondary_container/on_secondary_container, tertiary, error/on_error,
/// surface, surface_container_{lowest,low,,high,highest}, on_surface,
/// on_surface_variant, outline_variant (outline unused here — see the
/// close-matrix note).
pub fn stylesheet(s: &Scheme) -> String {
    format!(
        "window {{ background-color: {surface}; color: {on_surface}; }}\n\
         label {{ color: {on_surface}; }}\n\
         label.dim {{ color: {on_surface_variant}; }}\n\
         label.error-text {{ color: {error}; }}\n\
         headerbar {{ background-color: {surface}; color: {on_surface}; border-bottom: 1px solid {outline_variant}; }}\n\
         headerbar button {{ background-color: {surface_container_high}; color: {on_surface}; border-radius: 14px; padding: 4px 10px; border: none; }}\n\
         entry {{ background-color: {surface_container_highest}; color: {on_surface}; border-radius: 10px; padding: 6px 8px; border: 1px solid {outline_variant}; }}\n\
         entry:focus {{ border: 1px solid {primary}; }}\n\
         button {{ background-color: {primary_container}; color: {on_primary_container}; border-radius: 14px; padding: 5px 12px; border: none; }}\n\
         button:hover {{ background-color: {surface_container_high}; }}\n\
         list {{ background-color: {surface}; }}\n\
         list > row {{ background-color: {surface_container_low}; border-radius: 12px; padding: 4px 6px; margin: 3px 1px; }}\n\
         list > row:hover {{ background-color: {surface_container}; }}\n\
         progressbar > trough {{ background-color: {surface_container_highest}; border-radius: 6px; min-height: 8px; }}\n\
         progressbar > trough > progress {{ background-color: {primary}; border-radius: 6px; }}\n\
         checkbutton {{ color: {on_surface}; }}\n\
         spinbutton {{ background-color: {surface_container_highest}; color: {on_surface}; border: 1px solid {outline_variant}; }}\n\
         label.chip-active {{ color: {primary}; }}\n\
         label.chip-done {{ color: {tertiary}; }}\n\
         label.chip-failed {{ color: {error}; }}\n\
         label.chip-queued {{ color: {on_surface_variant}; }}\n\
         label.chip-verify {{ color: {on_secondary_container}; }}\n\
         label.chip-probe {{ color: {on_secondary_container}; }}\n\
         label.chip-seed {{ color: {primary}; }}\n",
        surface = hex_css(s.surface),
        on_surface = hex_css(s.on_surface),
        on_surface_variant = hex_css(s.on_surface_variant),
        error = hex_css(s.error),
        outline_variant = hex_css(s.outline_variant),
        surface_container_high = hex_css(s.surface_container_high),
        surface_container_highest = hex_css(s.surface_container_highest),
        primary = hex_css(s.primary),
        primary_container = hex_css(s.primary_container),
        on_primary_container = hex_css(s.on_primary_container),
        surface_container_low = hex_css(s.surface_container_low),
        surface_container = hex_css(s.surface_container),
        tertiary = hex_css(s.tertiary),
        on_secondary_container = hex_css(s.on_secondary_container),
    )
}

/// Settings Apply — the pure logic of the Slint handler
/// (`crates/velocitydm-slint/src/main.rs` on_apply_settings) so the
/// clamps and parses are unit-testable headless. The caller starts from
/// the LIVE engine config, so fields this panel does not expose
/// (`user_agent`, scheduler details) survive Apply.
// The settings form is inherently a flat field list — 9 typed params is
// the honest shape (a struct would just move the count elsewhere).
#[allow(clippy::too_many_arguments)]
pub fn apply_settings_fields(
    cfg: &mut EngineConfig,
    dir: &str,
    connections: i32,
    max_active: i32,
    limit: &str,
    proxy: &str,
    stop_start: &str,
    stop_end: &str,
    shutdown_when_done: bool,
) {
    let dir = dir.trim();
    if !dir.is_empty() {
        cfg.download_dir = PathBuf::from(dir);
    }
    cfg.default_connections = connections.clamp(1, 32) as usize;
    cfg.scheduler.max_active = max_active.clamp(1, 16) as usize;
    cfg.global_limit_bps = parse_bytes(limit.trim()).unwrap_or(0);
    let proxy = proxy.trim();
    if proxy.is_empty() || proxy.eq_ignore_ascii_case("none") {
        cfg.proxy = None;
    } else if let Ok(p) = ProxyConfig::parse_url(proxy) {
        cfg.proxy = Some(p);
    }
    if vdm_core::engine::scheduler::parse_hhmm(stop_start).is_some()
        && vdm_core::engine::scheduler::parse_hhmm(stop_end).is_some()
    {
        cfg.scheduler.stop_window =
            Some((stop_start.trim().to_string(), stop_end.trim().to_string()));
    }
    cfg.scheduler.shutdown_when_done = shutdown_when_done;
}

#[cfg(test)]
mod tests {
    use super::*;
    use vdm_md3::DEFAULT_SEED;

    #[test]
    fn chip_mapping_is_exhaustive() {
        // Every TaskState variant maps to a chip (match is exhaustive by
        // construction; this pins the exact text/class pairs).
        let cases = [
            (TaskState::Queued, ("queued", "chip-queued")),
            (TaskState::Probing, ("probe", "chip-probe")),
            (TaskState::Downloading, ("active", "chip-active")),
            (TaskState::Paused, ("queued", "chip-queued")),
            (TaskState::Verifying, ("verify", "chip-verify")),
            (TaskState::Completed, ("done", "chip-done")),
            (TaskState::Failed, ("failed", "chip-failed")),
            (TaskState::Seeding, ("seed", "chip-seed")),
        ];
        for (state, want) in cases {
            assert_eq!(state_chip(state), want, "state {state:?}");
        }
    }

    #[test]
    fn chip_visibility_pairs_with_states() {
        assert!(can_pause(TaskState::Downloading));
        assert!(can_pause(TaskState::Probing));
        assert!(can_pause(TaskState::Queued));
        assert!(!can_pause(TaskState::Paused));
        assert!(can_resume(TaskState::Paused));
        assert!(can_resume(TaskState::Failed));
        assert!(!can_resume(TaskState::Downloading));
    }

    #[test]
    fn stylesheet_is_token_only_and_contains_scheme_hex() {
        let s = Scheme::from_seed_hex(DEFAULT_SEED, true);
        let css = stylesheet(&s);
        assert!(css.contains(&hex_css(s.primary)));
        assert!(css.contains(&hex_css(s.error)));
        assert!(css.contains(&hex_css(s.surface)));
        assert!(css.contains("label.chip-active"));
        assert!(css.contains("progressbar > trough > progress"));
        // No runtime-string channel: the input is a Scheme (u32 fields),
        // so no task name/path could appear — pin the load_from_data
        // contract by asserting the stylesheet is pure ASCII CSS.
        assert!(css.is_ascii());
        // Same seed/dark flip the provider reload performs.
        let light = stylesheet(&Scheme::from_seed_hex(DEFAULT_SEED, false));
        assert!(light.contains(&hex_css(Scheme::from_seed_hex(DEFAULT_SEED, false).primary)));
    }

    #[test]
    fn display_text_is_literal_and_control_free() {
        // Markup payloads pass through literally: set_text never parses
        // Pango markup (the spoofing/blanking class stays unreachable).
        let payload = "<span foreground=\"red\">x</span>";
        assert_eq!(display_text(payload), payload);
        assert_eq!(display_text("na\u{7}me\n"), "na me");
        assert_eq!(display_text("a\tb\u{0}c"), "a b c");
        assert_eq!(display_text("  padded  "), "padded");
    }

    #[test]
    fn progress_fraction_clamps() {
        assert_eq!(progress_fraction(0, 100), 0.0);
        assert!((progress_fraction(50, 100) - 0.5).abs() < 1e-9);
        assert_eq!(progress_fraction(200, 100), 1.0);
        assert_eq!(progress_fraction(10, 0), 0.0);
    }

    #[test]
    fn proxy_display_formats_both_variants() {
        let http = ProxyConfig::parse_url("http://127.0.0.1:8080").unwrap();
        assert_eq!(proxy_display(&http), "http://127.0.0.1:8080");
        let socks = ProxyConfig::parse_url("socks5://10.0.0.9:1080").unwrap();
        assert_eq!(proxy_display(&socks), "socks5://10.0.0.9:1080");
    }

    #[test]
    fn settings_apply_clamps_and_parses() {
        let mut cfg = EngineConfig {
            user_agent: "VelocityDM/1.0".to_string(),
            ..Default::default()
        };
        let user_agent = cfg.user_agent.clone();
        apply_settings_fields(
            &mut cfg,
            "/tmp/dl",
            999,
            0,
            "2M",
            "socks5://127.0.0.1:1080",
            "23:30",
            "07:00",
            true,
        );
        assert_eq!(cfg.download_dir, PathBuf::from("/tmp/dl"));
        assert_eq!(cfg.default_connections, 32); // clamped 1..=32
        assert_eq!(cfg.scheduler.max_active, 1); // clamped 1..=16
        assert_eq!(cfg.global_limit_bps, 2_000_000);
        assert!(cfg.proxy.is_some());
        assert_eq!(
            cfg.scheduler.stop_window,
            Some(("23:30".to_string(), "07:00".to_string()))
        );
        assert!(cfg.scheduler.shutdown_when_done);
        assert_eq!(cfg.user_agent, user_agent); // unexposed field survives

        // "none" clears the proxy; bad parses are ignored (tolerant).
        apply_settings_fields(&mut cfg, "", 0, 0, "", "none", "bad", "bad", false);
        assert!(cfg.proxy.is_none());
        assert_eq!(
            cfg.scheduler.stop_window,
            Some(("23:30".to_string(), "07:00".to_string()))
        );
        assert_eq!(cfg.default_connections, 1); // clamp floor
        assert!(!cfg.scheduler.shutdown_when_done);
    }
}
