//! Shared `config.json` / `ui.json` handling — the gtk3/config.rs mirror
//! (same files, same fields, same tolerant-load semantics; one source of
//! truth for the on-disk format lives in the engine's serde types).

#![deny(unsafe_code)]

use vdm_core::engine::EngineConfig;

pub fn config_path(portable: bool) -> std::path::PathBuf {
    if portable {
        let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
        return exe
            .parent()
            .map(|p| p.join("velocitydm.config.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("velocitydm.config.json"));
    }
    let dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .filter(|v| !v.is_empty())
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    dir.join("velocitydm").join("config.json")
}

pub fn parse_config(text: &str) -> Option<EngineConfig> {
    serde_json::from_str(text).ok()
}

pub fn load_config(portable: bool) -> EngineConfig {
    let path = config_path(portable);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| parse_config(&t))
        .unwrap_or_default()
}

pub fn save_config(cfg: &EngineConfig, portable: bool) {
    let path = config_path(portable);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(&path, text);
    }
}

/// UI preferences persisted beside the config (`ui.json` in the same
/// directory; `velocitydm.ui.json` beside a portable binary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UiPrefs {
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
    match cfg.parent() {
        Some(dir) => dir.join("ui.json"),
        None => std::path::PathBuf::from("velocitydm.ui.json"),
    }
}

pub fn parse_prefs(text: &str) -> Option<UiPrefs> {
    serde_json::from_str(text).ok()
}

pub fn load_prefs(portable: bool) -> UiPrefs {
    std::fs::read_to_string(prefs_path(portable))
        .ok()
        .and_then(|t| parse_prefs(&t))
        .unwrap_or_default()
}

pub fn save_prefs(prefs: &UiPrefs, portable: bool) {
    if let Ok(text) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(prefs_path(portable), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_round_trip() {
        let p = UiPrefs {
            seed: 0x00639B,
            dark: false,
        };
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(parse_prefs(&text).unwrap(), p);
    }

    #[test]
    fn prefs_tolerant_parse() {
        for bad in ["", "not json", "{\"seed\": \"x\"}", "null"] {
            assert!(parse_prefs(bad).is_none(), "{bad:?}");
        }
        let fallback = load_prefs(true); // portable path: tempdir-free, just reads
        let _ = fallback;
        assert_eq!(UiPrefs::default().seed, vdm_md3::DEFAULT_SEED);
    }

    #[test]
    fn config_tolerant_parse() {
        assert!(parse_config("garbage").is_none());
        assert!(parse_config("null").is_none());
        assert!(parse_config("{").is_none());
        // a full round trip keeps engine fields
        let cfg: EngineConfig = Default::default();
        let text = serde_json::to_string(&cfg).unwrap();
        let back = parse_config(&text).unwrap();
        assert_eq!(back.global_limit_bps, cfg.global_limit_bps);
        assert_eq!(back.default_connections, cfg.default_connections);
    }
}
