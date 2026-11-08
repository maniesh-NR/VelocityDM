//! TLS connector (rustls with the system-independent webpki root store).

use crate::error::{Error, Result};
use std::net::TcpStream;
use std::sync::Arc;

pub type TlsStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

/// Wrap an established TCP stream in a TLS client session for `host`.
pub fn wrap(tcp: TcpStream, host: &str) -> Result<TlsStream> {
    let _ = tcp.set_nodelay(true);
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Http(format!("tls versions: {}", e)))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| Error::Http(format!("invalid TLS server name '{}'", host)))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), server_name)?;
    Ok(rustls::StreamOwned::new(conn, tcp))
}
