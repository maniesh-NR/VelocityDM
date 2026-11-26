//! LAN-mode secrets: bearer token + self-signed TLS certificate material.
//!
//! Feature-free on purpose (S8-W1 spec rule 2/6): the file format, the
//! 0600-at-creation discipline, token generation and rotation never need
//! the crypto-heavy `lan` feature — only certificate GENERATION and the
//! TLS accept path do (`lan.rs`, `#[cfg(feature = "lan")]`). The win7
//! mirror compiles this module untouched (a non-loopback bind is refused
//! at startup there — see `RpcServer::start_ex`).
//!
//! The secrets live in their OWN file, never inside `EngineConfig`/
//! `config.json`: the daemon rewrites the engine config on every
//! `ConfigChanged` event (`crates/velocitydm/src/daemon.rs`), and that
//! round-trip drops unknown fields — a token stored there would be
//! silently destroyed.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

/// The bearer token byte length (hex-encoded twice that on the wire).
pub const TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LanSecrets {
    /// 64-char lowercase hex bearer token.
    #[serde(default)]
    pub token: String,
    /// Self-signed certificate PEM (empty until the `lan` feature
    /// generates it at the first non-loopback start).
    #[serde(default)]
    pub cert: String,
    /// Certificate private key PEM.
    #[serde(default)]
    pub key: String,
    /// SHA-256 fingerprint of the certificate DER (hex) — the value
    /// remote clients pin (TOFU); printed by the daemon/CLI for
    /// out-of-band comparison (gate 0018 ATKR-5).
    #[serde(default)]
    pub fingerprint: String,
}

/// Where the LAN secrets file lives: `$VELOCITYDM_LAN_SECRETS` (test
/// seam + operational override) → portable: exe dir →
/// `$XDG_CONFIG_HOME|$HOME/.config/velocitydm/`, file
/// `velocitydm-lan-secrets.json` (sibling of `config.json`).
pub fn secrets_path(portable: bool) -> PathBuf {
    if let Ok(p) = std::env::var("VELOCITYDM_LAN_SECRETS") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if portable {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join("velocitydm-lan-secrets.json");
            }
        }
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("velocitydm").join("velocitydm-lan-secrets.json")
}

/// Load the secrets file; missing/partial entries are filled in: the
/// token whenever absent, the certificate only via the `lan` feature's
/// generator (a feature-free caller — the CLI's `lan-token` — passes
/// `certs = false` and never touches cert fields).
pub fn load_or_generate(portable: bool, with_cert: bool) -> Result<LanSecrets, String> {
    let path = secrets_path(portable);
    let mut secrets: LanSecrets = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let mut changed = false;
    if secrets.token.trim().len() != TOKEN_BYTES * 2 {
        secrets.token = generate_token_hex();
        changed = true;
    }
    if with_cert && secrets.cert.trim().is_empty() {
        #[cfg(feature = "lan")]
        {
            let (cert, key) = crate::lan::generate_certificate()?;
            secrets.cert = cert;
            secrets.key = key;
            changed = true;
        }
        #[cfg(not(feature = "lan"))]
        {
            return Err("certificate generation requires the `lan` feature \
                 (rebuild vdm-rpc with --features lan)"
                .to_string());
        }
    }
    // backfill the fingerprint for certs generated before the field
    // existed (or hand-migrated secrets files)
    #[cfg(feature = "lan")]
    if !secrets.cert.trim().is_empty() && secrets.fingerprint.trim().is_empty() {
        secrets.fingerprint = crate::lan::certificate_fingerprint(&secrets.cert)?;
        changed = true;
    }
    if changed || !path.exists() {
        save(&secrets, &path)?;
    }
    Ok(secrets)
}

/// Replace the token with a fresh random one (certificate untouched —
/// TOFU pins survive rotation) and persist.
pub fn rotate_token(portable: bool) -> Result<String, String> {
    let path = secrets_path(portable);
    let mut secrets: LanSecrets = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    secrets.token = generate_token_hex();
    save(&secrets, &path)?;
    Ok(secrets.token)
}

/// Write with 0600 on Unix at creation (best-effort chmod on rewrite:
/// an existing file keeps its mode; creation always goes through
/// `OpenOptions` with `mode(0o600)` so the token never lands world- or
/// group-readable). Windows relies on the user-scoped profile ACLs.
fn save(secrets: &LanSecrets, path: &PathBuf) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    }
    let text = serde_json::to_string_pretty(secrets).map_err(|e| format!("serialize: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
        f.write_all(text.as_bytes())
            .map_err(|e| format!("write {}: {}", path.display(), e))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, text.as_bytes())
            .map_err(|e| format!("write {}: {}", path.display(), e))?;
    }
    Ok(())
}

/// Constant-time bearer-token check: XOR-fold over the fixed-length
/// lowercase-hex token. The length comparison leaks only the length
/// (the token is a fixed 64 hex chars; anything else is simply wrong).
pub fn token_matches(presented: &str, expected: &str) -> bool {
    let a = presented.trim();
    let b = expected.trim();
    if a.len() != TOKEN_BYTES * 2 || b.len() != TOKEN_BYTES * 2 {
        return false;
    }
    let mut diff: u8 = (a.len() ^ b.len()) as u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn generate_token_hex() -> String {
    let bytes = vdm_core::util::random_bytes(TOKEN_BYTES);
    let mut hexed = String::with_capacity(TOKEN_BYTES * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(hexed, "{:02x}", b);
    }
    hexed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_match_is_exact_and_length_insensitive() {
        let tok = "a".repeat(64);
        assert!(token_matches(&tok.clone(), &tok.clone()));
        let mut other = tok.clone();
        other.replace_range(7..8, "b");
        assert!(!token_matches(&other, &tok));
        assert!(!token_matches("", &tok));
        assert!(!token_matches(&"a".repeat(63), &tok));
        // whitespace around the header value is tolerated (trimmed)
        assert!(token_matches(&format!("  {} ", tok), &tok));
    }

    #[test]
    fn secrets_roundtrip_and_partial_fill() {
        let dir = std::env::temp_dir().join(format!(
            "vdm-lan-secrets-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("secrets.json");
        std::env::set_var("VELOCITYDM_LAN_SECRETS", &file);
        // generate token only (feature-free path, with_cert = false)
        let s = load_or_generate(false, false).unwrap();
        assert_eq!(s.token.len(), 64);
        assert!(s.cert.is_empty());
        assert!(file.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // partial file keeps its token on reload
        let s2 = load_or_generate(false, false).unwrap();
        assert_eq!(s2.token, s.token);
        // rotation keeps (empty) cert and changes the token
        let rotated = rotate_token(false).unwrap();
        assert_ne!(rotated, s.token);
        assert_eq!(rotated.len(), 64);
        let s3 = load_or_generate(false, false).unwrap();
        assert_eq!(s3.token, rotated);
        std::env::remove_var("VELOCITYDM_LAN_SECRETS");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
