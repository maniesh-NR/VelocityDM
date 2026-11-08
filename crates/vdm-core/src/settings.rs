// SPDX-License-Identifier: GPL-3.0-or-later
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{Result, VdmError};

/// Daemon settings. Loaded from `<config>/settings.json`; in portable mode
/// the whole data directory lives next to the executable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub rpc_host: String,
    pub rpc_port: u16,
    pub sse_port: u16,
    pub download_dir: PathBuf,
    pub max_concurrent_tasks: usize,
    pub default_segments: u32,
    pub max_connections_per_host: u32,
    /// Global rate limit in bytes/sec; 0 = unlimited.
    pub global_rate_limit_bps: u64,
    pub user_agent: String,
    /// UI refresh rate for progress snapshots; clamped to 10..=30 Hz.
    pub refresh_hz: u32,
    /// Parallel segmentation only above this file size (bytes).
    pub min_segment_bytes: u64,
    pub resume_on_start: bool,
    pub extra_headers: Vec<(String, String)>,
}

impl Default for Settings {
    fn default() -> Self {
        let download_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Downloads");
        Settings {
            rpc_host: "127.0.0.1".into(),
            rpc_port: crate::DEFAULT_RPC_PORT,
            sse_port: crate::DEFAULT_SSE_PORT,
            download_dir,
            max_concurrent_tasks: 3,
            default_segments: 8,
            max_connections_per_host: 8,
            global_rate_limit_bps: 0,
            user_agent: format!("VelocityDM/{}", crate::VERSION),
            refresh_hz: 12,
            min_segment_bytes: 8 * 1024 * 1024,
            resume_on_start: true,
            extra_headers: Vec::new(),
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.refresh_hz = self.refresh_hz.clamp(10, 30);
        if self.rpc_port == 0 {
            self.rpc_port = crate::DEFAULT_RPC_PORT;
        }
        if self.sse_port == 0 {
            self.sse_port = crate::DEFAULT_SSE_PORT;
        }
        if self.max_concurrent_tasks == 0 {
            self.max_concurrent_tasks = 1;
        }
        if self.default_segments == 0 {
            self.default_segments = 1;
        }
        if self.max_connections_per_host == 0 {
            self.max_connections_per_host = 1;
        }
        self
    }

    pub fn load(path: &Path) -> Result<Settings> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| VdmError::Io(std::io::Error::new(e.kind(), format!("settings: {}", e))))?;
        let parsed: Settings = serde_json::from_str(&text)
            .map_err(|e| VdmError::InvalidInput(format!("settings parse: {}", e)))?;
        Ok(parsed.sanitized())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Resolves the data directory: portable mode keeps everything next to the
    /// executable, otherwise XDG-style locations are used.
    pub fn resolve_dirs(portable: bool, data_dir: Option<PathBuf>) -> (PathBuf, PathBuf) {
        if portable {
            let base = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("."));
            let dir = base.join("velocitydm-data");
            return (dir.clone(), dir.join("settings.json"));
        }
        if let Some(dir) = data_dir {
            return (dir.clone(), dir.join("settings.json"));
        }
        let config = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("velocitydm");
        let data = dirs::data_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".local/share"))
            .join("velocitydm");
        (data, config.join("settings.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_hz_is_clamped() {
        let mut s = Settings::default();
        s.refresh_hz = 90;
        assert_eq!(s.clone().sanitized().refresh_hz, 30);
        s.refresh_hz = 2;
        assert_eq!(s.clone().sanitized().refresh_hz, 10);
        s.refresh_hz = 24;
        assert_eq!(s.clone().sanitized().refresh_hz, 24);
    }

    #[test]
    fn settings_roundtrip() {
        let dir = std::env::temp_dir().join(format!("vdm-settings-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        s.refresh_hz = 20;
        s.save(&path).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.refresh_hz, 20);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn portable_dirs_live_next_to_exe() {
        let (data, cfg) = Settings::resolve_dirs(true, None);
        assert!(cfg.ends_with("settings.json"));
        assert!(data.to_string_lossy().contains("velocitydm-data"));
    }
}
