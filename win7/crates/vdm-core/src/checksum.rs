//! Checksum verification (IDM-style final integrity check).

use crate::error::{Error, Result};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algo {
    Md5,
    Sha1,
    Sha256,
}

impl Algo {
    pub fn parse(s: &str) -> Result<Algo> {
        match s.to_ascii_lowercase().as_str() {
            "md5" => Ok(Algo::Md5),
            "sha1" | "sha-1" => Ok(Algo::Sha1),
            "sha256" | "sha-256" => Ok(Algo::Sha256),
            other => Err(Error::Task(format!(
                "unsupported checksum algo '{}'",
                other
            ))),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
        }
    }
}

/// Digest a file, reporting progress through the callback (bytes done, total).
pub fn digest_file(path: &Path, algo: Algo, progress: impl Fn(u64, u64)) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let total = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut done = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    let mut hex_out = String::with_capacity(64);
    match algo {
        Algo::Md5 => hash_loop::<Md5>(&mut f, &mut buf, total, &mut done, &progress, &mut hex_out)?,
        Algo::Sha1 => {
            hash_loop::<Sha1>(&mut f, &mut buf, total, &mut done, &progress, &mut hex_out)?
        }
        Algo::Sha256 => {
            hash_loop::<Sha256>(&mut f, &mut buf, total, &mut done, &progress, &mut hex_out)?
        }
    }
    Ok(hex_out)
}

fn hash_loop<D: Digest + Default>(
    f: &mut std::fs::File,
    buf: &mut [u8],
    total: u64,
    done: &mut u64,
    progress: &impl Fn(u64, u64),
    hex_out: &mut String,
) -> Result<()> {
    let mut d = D::default();
    loop {
        let n = f.read(buf)?;
        if n == 0 {
            break;
        }
        d.update(&buf[..n]);
        *done += n as u64;
        progress(*done, total);
    }
    let out = d.finalize();
    *hex_out = hex::encode(out);
    Ok(())
}

/// Verify a file against an expected hex digest.
pub fn verify_file(path: &Path, algo: Algo, expected_hex: &str) -> Result<bool> {
    let expected = expected_hex.trim().to_ascii_lowercase();
    if hex::decode(&expected).is_err() {
        return Err(Error::Task("expected checksum is not valid hex".into()));
    }
    let actual = digest_file(path, algo, |_, _| {})?;
    Ok(actual == expected)
}

/// Digest in-memory bytes (used by tests and the BT engine for piece hashes).
pub fn digest_bytes(algo: Algo, data: &[u8]) -> String {
    match algo {
        Algo::Md5 => hex::encode(Md5::digest(data)),
        Algo::Sha1 => hex::encode(Sha1::digest(data)),
        Algo::Sha256 => hex::encode(Sha256::digest(data)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(
            digest_bytes(Algo::Md5, b"hello"),
            "5d41402abc4b2a76b9719d911017c592"
        );
        assert_eq!(
            digest_bytes(Algo::Sha1, b"hello"),
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        );
        assert_eq!(
            digest_bytes(Algo::Sha256, b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn file_digest_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.bin");
        std::fs::write(&p, b"hello world").unwrap();
        let sum = digest_file(&p, Algo::Sha256, |_, _| {}).unwrap();
        assert!(verify_file(&p, Algo::Sha256, &sum).unwrap());
        assert!(!verify_file(&p, Algo::Sha256, &format!("{}{}", sum, "00")).unwrap());
        assert!(verify_file(&p, Algo::Sha256, &sum.to_uppercase()).unwrap());
        assert!(verify_file(&p, Algo::Sha256, "zz").is_err());
    }

    #[test]
    fn algo_parse() {
        assert_eq!(Algo::parse("MD5").unwrap(), Algo::Md5);
        assert_eq!(Algo::parse("sha-256").unwrap(), Algo::Sha256);
        assert!(Algo::parse("crc32").is_err());
    }
}
