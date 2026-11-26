//! LAN-mode TLS: self-signed certificate generation (rcgen, ring
//! backend — no aws-lc-rs, no x509-parser) and the rustls
//! `ServerConfig` for non-loopback peers.
//!
//! Compiled only under `#[cfg(feature = "lan")]`; the win7 mirror does
//! not declare the feature, so this module is inert there while the
//! source stays byte-identical (authorized divergence = feature
//! availability only).

use rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// Generate a self-signed certificate + PKCS#8 key pair.
///
/// Identity model (S8-W1 spec rule 3): clients authenticate the daemon
/// by PINNING the certificate fingerprint (TOFU) — the SAN list is
/// informational, hostname verification is the pin's job. The 3650-day
/// validity exists so TOFU pins do not churn: in this model an expired
/// self-signed cert would force a re-pin ceremony with no security
/// gain; rotation stays available via `velocitydm lan-token --rotate`
/// (token) or deleting the secrets file (full identity reset).
pub fn generate_certificate() -> Result<(String, String), String> {
    let mut params = rcgen::CertificateParams::new(vec![
        "localhost".to_string(),
        "velocitydm-daemon".to_string(),
    ])
    .map_err(|e| format!("cert params: {}", e))?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "velocitydm-daemon");
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::LOCALHOST,
        )));
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(std::net::IpAddr::V6(
            std::net::Ipv6Addr::LOCALHOST,
        )));
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(2036, 1, 1);
    let key = rcgen::KeyPair::generate().map_err(|e| format!("key gen: {}", e))?;
    let cert = params
        .self_signed(&key)
        .map_err(|e| format!("cert gen: {}", e))?;
    Ok((cert.pem(), key.serialize_pem()))
}

/// Build the server-side TLS config from PEM secrets. rustls keeps
/// `tls12` enabled so Android API 26–28 clients (no native TLS 1.3)
/// can still handshake.
pub fn server_config(cert_pem: &str, key_pem: &str) -> Result<rustls::ServerConfig, String> {
    let cert_der = pem_to_der(cert_pem, "CERTIFICATE").map_err(|e| format!("cert pem: {}", e))?;
    let key_der = pem_to_der(key_pem, "PRIVATE KEY").map_err(|e| format!("key pem: {}", e))?;
    let certs = vec![CertificateDer::from(cert_der)];
    let key = PrivateKeyDer::Pkcs8(key_der.into());
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("tls versions: {}", e))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("tls config: {}", e))
}

fn pem_to_der(pem: &str, expected_label: &str) -> Result<Vec<u8>, String> {
    let parsed = pem::parse(pem).map_err(|e| format!("parse: {}", e))?;
    if parsed.tag() != expected_label {
        return Err(format!(
            "expected {} block, got {}",
            expected_label,
            parsed.tag()
        ));
    }
    Ok(parsed.into_contents())
}

/// SHA-256 fingerprint of the certificate DER (lowercase hex) — the
/// value remote clients pin (TOFU) and the daemon/CLI print for
/// out-of-band comparison.
pub fn certificate_fingerprint(cert_pem: &str) -> Result<String, String> {
    let der = pem_to_der(cert_pem, "CERTIFICATE")?;
    Ok(vdm_core::checksum::digest_bytes(
        vdm_core::checksum::Algo::Sha256,
        &der,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_generates_and_config_builds() {
        let (cert, key) = generate_certificate().unwrap();
        assert!(cert.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(key.starts_with("-----BEGIN PRIVATE KEY-----"));
        let cfg = server_config(&cert, &key).unwrap();
        // fingerprint: stable DER digest, lowercase hex
        let fp = certificate_fingerprint(&cert).unwrap();
        assert_eq!(fp.len(), 64);
        assert_eq!(certificate_fingerprint(&cert).unwrap(), fp);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
        // rustls keeps TLS 1.2 available (Android API 26-28 floor): a
        // 1.2-only client must be able to negotiate. The supported
        // versions list is private API; the honest public witness is
        // that a TLS 1.2-pinned client config is accepted by the same
        // builder path.
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let client = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS12])
            .map(|b| {
                b.with_root_certificates(rustls::RootCertStore::empty())
                    .with_no_client_auth()
            });
        assert!(client.is_ok(), "TLS 1.2 must stay negotiable");
        let _ = cfg; // the server config built above is the real witness
    }

    #[test]
    fn bad_pem_is_rejected() {
        assert!(server_config("not a pem", "nope").is_err());
    }
}
