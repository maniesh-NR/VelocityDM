//! Shared configuration files — this frontend reads/writes the SAME
//! `config.json` (engine) and `ui.json` (Material You seed + mode) as
//! the daemon, the egui GUI and the Slint frontend, so all frontends on
//! one machine agree.
//!
//! Mirrors `crates/velocitydm/src/daemon.rs` and
//! `crates/velocitydm-slint/src/config.rs` (binary crates cannot be
//! imported as libraries). Loading is tolerant: any read or serde
//! failure degrades to the defaults, never panics (the `parse_*`
//! helpers exist so that behavior is unit-testable headless).

use vdm_core::engine::EngineConfig;

pub fn config_path(portable: bool) -> std::path::PathBuf {
    if portable {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join("velocitydm.config.json");
            }
        }
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    base.join("velocitydm").join("config.json")
}

pub fn parse_config(text: &str) -> Option<EngineConfig> {
    serde_json::from_str(text).ok()
}

pub fn load_config(portable: bool) -> EngineConfig {
    let path = config_path(portable);
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Some(cfg) = parse_config(&text) {
            return cfg;
        }
    }
    EngineConfig::default()
}

pub fn save_config(cfg: &EngineConfig, portable: bool) {
    let path = config_path(portable);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, text);
    }
}

/// Material You UI preferences (seed + light/dark), same file as the
/// egui GUI's `theme::UiPrefs` and the Slint frontend's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UiPrefs {
    /// Seed color, 0xRRGGBB.
    pub seed: u32,
    pub dark: bool,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            seed: vdm_md3::DEFAULT_SEED,
            dark: true,
        }
    }
}

fn prefs_path(portable: bool) -> std::path::PathBuf {
    let cfg = config_path(portable);
    cfg.parent()
        .map(|d| d.join("ui.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("velocitydm.ui.json"))
}

pub fn parse_prefs(text: &str) -> Option<UiPrefs> {
    serde_json::from_str(text).ok()
}

pub fn load_prefs(portable: bool) -> UiPrefs {
    if let Ok(text) = std::fs::read_to_string(prefs_path(portable)) {
        if let Some(p) = parse_prefs(&text) {
            return p;
        }
    }
    UiPrefs::default()
}

pub fn save_prefs(p: &UiPrefs, portable: bool) {
    let path = prefs_path(portable);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(p) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_round_trip() {
        let p = UiPrefs {
            seed: 0x38_6A_33,
            dark: false,
        };
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(parse_prefs(&text), Some(p));
    }

    #[test]
    fn prefs_tolerant_parse() {
        // Hostile/garbage ui.json must degrade to defaults, never panic.
        assert_eq!(parse_prefs(""), None);
        assert_eq!(parse_prefs("not json"), None);
        assert_eq!(parse_prefs("{\"seed\": \"x\"}"), None);
        assert_eq!(parse_prefs("null"), None);
        let fallback = parse_prefs("garbage").unwrap_or_else(UiPrefs::default);
        assert_eq!(fallback, UiPrefs::default());
        assert_eq!(fallback.seed, vdm_md3::DEFAULT_SEED);
    }

    #[test]
    fn config_tolerant_parse() {
        assert_eq!(parse_config("garbage"), None);
        // Unknown keys are additive-tolerant on the core's serde, but a
        // structurally broken file yields None -> default config.
        assert_eq!(parse_config("[]"), None);
    }

    #[test]
    fn config_round_trip_keeps_engine_fields() {
        let mut cfg = EngineConfig::default();
        cfg.global_limit_bps = 250_000;
        cfg.default_connections = 9;
        let text = serde_json::to_string(&cfg).unwrap();
        let parsed = parse_config(&text).unwrap();
        assert_eq!(parsed.global_limit_bps, 250_000);
        assert_eq!(parsed.default_connections, 9);
    }
}
