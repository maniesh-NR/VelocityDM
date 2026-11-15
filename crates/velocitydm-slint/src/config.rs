//! Shared configuration files — this frontend reads/writes the SAME
//! `config.json` (engine) and `ui.json` (Material You seed + mode) as
//! the daemon and the egui GUI, so all frontends on one machine agree.
//!
//! Mirrors `crates/velocitydm/src/daemon.rs` (the binary crate cannot
//! be imported as a library).

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

pub fn load_config(portable: bool) -> EngineConfig {
    let path = config_path(portable);
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<EngineConfig>(&text) {
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
/// egui GUI's `theme::UiPrefs`.
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

pub fn load_prefs(portable: bool) -> UiPrefs {
    if let Ok(text) = std::fs::read_to_string(prefs_path(portable)) {
        if let Ok(p) = serde_json::from_str::<UiPrefs>(&text) {
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
