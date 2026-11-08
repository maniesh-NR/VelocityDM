//! `.part` / `.part.vdmctl` resume state for HTTP/HTTPS downloads (IDM-style
//! crash-safe resume).

use crate::range_pool::Interval;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeState {
    pub version: u32,
    pub url: String,
    pub mirrors: Vec<String>,
    pub size: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub checksum_algo: Option<String>,
    pub checksum_hex: Option<String>,
    pub filename: String,
    /// Remaining (not yet downloaded) byte intervals.
    pub remaining: Vec<Interval>,
}

impl ResumeState {
    pub fn part_path(out_path: &Path) -> PathBuf {
        let mut s = out_path.as_os_str().to_os_string();
        s.push(".part");
        PathBuf::from(s)
    }

    pub fn ctl_path(out_path: &Path) -> PathBuf {
        let mut s = out_path.as_os_str().to_os_string();
        s.push(".vdmctl");
        PathBuf::from(s)
    }

    pub fn save(&self, out_path: &Path) -> std::io::Result<()> {
        let path = Self::ctl_path(out_path);
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        // write via temp + rename for crash safety
        let tmp = path.with_extension("vdmctl.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)
    }

    pub fn load(out_path: &Path) -> Option<ResumeState> {
        let path = Self::ctl_path(out_path);
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn remove(out_path: &Path) {
        let _ = std::fs::remove_file(Self::ctl_path(out_path));
        let _ = std::fs::remove_file(Self::part_path(out_path));
    }

    /// Whether a saved state can be reused: same size and validator (etag or
    /// last-modified), or no validator at all.
    pub fn matches(
        &self,
        url: &str,
        size: u64,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> bool {
        if self.size != size {
            return false;
        }
        let same_validator = match (self.etag.as_deref(), etag) {
            (Some(a), Some(b)) => a == b,
            (None, None) => match (self.last_modified.as_deref(), last_modified) {
                (Some(a), Some(b)) => a == b,
                (None, None) => true,
                _ => false,
            },
            // missing etag on either side: fall back to last-modified
            (None, _) | (_, None) => match (self.last_modified.as_deref(), last_modified) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        };
        let _ = url;
        same_validator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("file.zip");
        let st = ResumeState {
            version: 1,
            url: "http://x/f".into(),
            mirrors: vec!["http://y/f".into()],
            size: 1000,
            etag: Some("abc".into()),
            last_modified: None,
            checksum_algo: Some("sha256".into()),
            checksum_hex: Some("00".into()),
            filename: "file.zip".into(),
            remaining: vec![Interval {
                start: 400,
                end: 999,
            }],
        };
        st.save(&out).unwrap();
        assert_eq!(
            ResumeState::part_path(&out),
            dir.path().join("file.zip.part")
        );
        let back = ResumeState::load(&out).unwrap();
        assert_eq!(back.url, "http://x/f");
        assert_eq!(back.remaining.len(), 1);
        assert_eq!(back.remaining[0].end, 999);
        ResumeState::remove(&out);
        assert!(ResumeState::load(&out).is_none());
    }

    #[test]
    fn match_validation() {
        let st = ResumeState {
            version: 1,
            url: String::new(),
            mirrors: vec![],
            size: 100,
            etag: Some("e1".into()),
            last_modified: None,
            checksum_algo: None,
            checksum_hex: None,
            filename: "f".into(),
            remaining: vec![],
        };
        assert!(st.matches("http://x/", 100, Some("e1"), None));
        assert!(!st.matches("http://x/", 100, Some("e2"), None));
        assert!(!st.matches("http://x/", 200, Some("e1"), None));
        let st2 = ResumeState {
            etag: None,
            last_modified: Some("date".into()),
            ..st.clone()
        };
        assert!(st2.matches("http://x/", 100, None, Some("date")));
        assert!(!st2.matches("http://x/", 100, None, Some("other")));
        let st3 = ResumeState {
            etag: None,
            last_modified: None,
            ..st.clone()
        };
        assert!(st3.matches("http://x/", 100, None, None));
    }
}
