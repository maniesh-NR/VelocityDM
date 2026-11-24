//! TLS connector (rustls with the system-independent webpki root store).
//! The `TlsMin` policy type is feature-free (it carries no TLS types) so
//! the settings surface exists in every build; the connector itself is
//! compiled under the `tls` feature.

use crate::error::{Error, Result};
#[cfg(feature = "tls")]
use std::net::TcpStream;
#[cfg(feature = "tls")]
use std::sync::Arc;

#[cfg(feature = "tls")]
pub type TlsStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

#[cfg(feature = "tls")]
static VERSIONS_TLS12: &[&rustls::SupportedProtocolVersion] =
    &[&rustls::version::TLS12, &rustls::version::TLS13];
#[cfg(feature = "tls")]
static VERSIONS_TLS13: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

// ---------------------------------------------------------------------------
// S11-i B22 — TLS minimum-version policy (docs/SPECS/S11-ADOPTIONS-WAVE4.md
// §B22; public-spec source: the rustls public version-policy API, no new
// crates — rustls+ring supports only TLS 1.2/1.3, so the knob is
// structurally floor-raising: lowering below the floor is unrepresentable).
// ---------------------------------------------------------------------------

/// The TLS minimum-version policy knob (`tlsMin` settings surface).
/// `Default` keeps rustls's safe defaults — zero behavior change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsMin {
    #[default]
    Default,
    Tls12,
    Tls13,
}

impl TlsMin {
    /// Parse the settings-surface knob value. `None` = unknown value —
    /// the caller applies the tolerant-load pattern (default + warn),
    /// so an unknown policy never reaches the connector at all: the
    /// fail-closed property of spec rule 20 holds structurally.
    pub fn parse(s: &str) -> Option<TlsMin> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" | "" => Some(TlsMin::Default),
            "tls12" | "1.2" => Some(TlsMin::Tls12),
            "tls13" | "1.3" => Some(TlsMin::Tls13),
            _ => None,
        }
    }

    /// The protocol-version selection for this policy (unit-testable;
    /// the wire-level refusal claim is docs-only — see the spec).
    #[cfg(feature = "tls")]
    pub fn protocol_versions(self) -> &'static [&'static rustls::SupportedProtocolVersion] {
        match self {
            TlsMin::Default => rustls::DEFAULT_VERSIONS,
            TlsMin::Tls12 => VERSIONS_TLS12,
            TlsMin::Tls13 => VERSIONS_TLS13,
        }
    }

    /// The canonical settings-surface spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            TlsMin::Default => "default",
            TlsMin::Tls12 => "tls12",
            TlsMin::Tls13 => "tls13",
        }
    }
}

/// Wrap an established TCP stream in a TLS client session for `host`
/// (rustls safe defaults — the pre-wave behavior, unchanged for callers
/// that carry no policy).
#[cfg(feature = "tls")]
pub fn wrap(tcp: TcpStream, host: &str) -> Result<TlsStream> {
    wrap_with_policy(tcp, host, TlsMin::Default)
}

/// B22: the policy-aware wrap.
#[cfg(feature = "tls")]
pub fn wrap_with_policy(tcp: TcpStream, host: &str, min: TlsMin) -> Result<TlsStream> {
    let _ = tcp.set_nodelay(true);
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(min.protocol_versions())
        .map_err(|e| Error::Http(format!("tls versions: {}", e)))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| Error::Http(format!("invalid TLS server name '{}'", host)))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), server_name)?;
    Ok(rustls::StreamOwned::new(conn, tcp))
}

#[cfg(all(test, feature = "tls"))]
mod tests {
    use super::*;

    #[test]
    fn b22_parse_matrix() {
        assert_eq!(TlsMin::parse("default"), Some(TlsMin::Default));
        assert_eq!(TlsMin::parse(""), Some(TlsMin::Default));
        assert_eq!(TlsMin::parse("TLS12"), Some(TlsMin::Tls12));
        assert_eq!(TlsMin::parse(" tls13 "), Some(TlsMin::Tls13));
        assert_eq!(TlsMin::parse("1.2"), Some(TlsMin::Tls12));
        assert_eq!(TlsMin::parse("1.3"), Some(TlsMin::Tls13));
        // unknown values: None — the tolerant caller warns + keeps default
        assert_eq!(TlsMin::parse("tls11"), None);
        assert_eq!(TlsMin::parse("1.0"), None);
        assert_eq!(TlsMin::parse("nonsense"), None);
    }

    #[test]
    fn b22_version_selection() {
        // default = rustls safe defaults (TLS 1.2 + 1.3): zero change
        assert_eq!(
            TlsMin::Default.protocol_versions(),
            rustls::DEFAULT_VERSIONS
        );
        // floor-raising only: the rustls+ring floor (TLS 1.2) is reachable,
        // nothing below it exists in the enum
        assert_eq!(TlsMin::Tls12.protocol_versions(), VERSIONS_TLS12);
        assert_eq!(TlsMin::Tls13.protocol_versions(), VERSIONS_TLS13);
        // canonical spellings round-trip through the settings surface
        for t in [TlsMin::Default, TlsMin::Tls12, TlsMin::Tls13] {
            assert_eq!(TlsMin::parse(t.as_str()), Some(t));
        }
    }
}
