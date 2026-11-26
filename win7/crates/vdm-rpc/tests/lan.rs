//! S8-W1 LAN-mode integration witnesses (compiled only with the `lan`
//! feature; the CI `Test (ubuntu-latest)` job runs
//! `cargo test -p vdm-rpc --features lan --locked`).
//!
//! Coverage (spec §6): the non-loopback leg binds a wildcard listener,
//! discovers the host's own non-loopback address (UDP-connect trick —
//! no packets leave the host), and drives a rustls client pinned to the
//! generated certificate through the token matrix; plain-HTTP bytes to
//! the TLS path are refused; loopback peers through the SAME wildcard
//! listener keep the byte-identical plain path; the connection budget
//! bounds non-loopback peers only; the header flood is capped for
//! non-loopback peers; secrets are stable across restarts.

#![cfg(feature = "lan")]

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::ClientConfig;
use std::io::{Read, Write};
use std::net::{TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vdm_core::engine::{Engine, EngineConfig};
use vdm_rpc::{secrets, RpcServer};

/// Serializes the tests that mutate `$VELOCITYDM_LAN_SECRETS` (the
/// secrets path is read once per server start).
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn test_engine() -> Arc<Engine> {
    let dir = std::env::temp_dir().join(format!(
        "vdm-rpc-lan-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    Engine::new(EngineConfig {
        download_dir: dir,
        ..Default::default()
    })
}

fn local_lan_ip() -> std::net::IpAddr {
    let s = UdpSocket::bind("0.0.0.0:0").expect("bind udp");
    s.connect("8.8.8.8:80")
        .expect("connect udp (no packets sent)");
    s.local_addr().expect("local addr").ip()
}

/// One HTTPS request over a fresh TLS connection that trusts exactly
/// the daemon's generated self-signed certificate (root-store pin —
/// a different server cert fails verification). Returns
/// (status, head+body as lossy text) or None when the connection is
/// dropped before a full response (refusal paths).
fn tls_request(
    addr: &str,
    cert_pem: &str,
    auth: Option<&str>,
    request: &str,
) -> Option<(u16, String)> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(cert_der(cert_pem))
        .expect("trust the generated cert");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = ServerName::try_from("localhost".to_string()).unwrap();
    let conn = rustls::ClientConnection::new(Arc::new(config), server_name).ok()?;
    let tcp = TcpStream::connect(addr).ok()?;
    tcp.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    tcp.set_write_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    let auth_line = auth
        .map(|t| format!("Authorization: Bearer {}\r\n", t))
        .unwrap_or_default();
    let full = request.replace("{AUTH}", &auth_line);
    tls.write_all(full.as_bytes()).ok()?;
    tls.flush().ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
        if buf.len() > 256 * 1024 {
            break;
        }
    }
    if buf.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = text
        .strip_prefix("HTTP/1.1 ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some((status, text))
}

fn cert_der(pem: &str) -> CertificateDer<'static> {
    let parsed = pem::parse(pem).expect("cert pem");
    CertificateDer::from(parsed.into_contents())
}

fn health_req() -> String {
    "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n{AUTH}Connection: close\r\n\r\n".to_string()
}

fn rpc_req(body: &str) -> String {
    format!(
        "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{{AUTH}}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn events_req() -> String {
    "GET /events HTTP/1.1\r\nHost: 127.0.0.1\r\n{AUTH}Connection: close\r\n\r\n".to_string()
}

fn plain_loopback_health(port: u16) -> String {
    let loopback = format!("127.0.0.1:{}", port);
    let mut plain = TcpStream::connect(&loopback).unwrap();
    plain
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    plain
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    plain.read_to_end(&mut buf).unwrap();
    String::from_utf8_lossy(&buf).to_string()
}

fn fresh_secrets_env(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vdm-lan-it-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("VELOCITYDM_LAN_SECRETS", dir.join("secrets.json"));
    dir
}

fn clear_secrets_env(dir: &std::path::PathBuf) {
    std::env::remove_var("VELOCITYDM_LAN_SECRETS");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lan_mode_full_matrix() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = fresh_secrets_env("matrix");
    let engine = test_engine();
    let server = RpcServer::start_ex(engine, "0.0.0.0:0", false).expect("wildcard start");
    let s = secrets::load_or_generate(false, false).expect("secrets");
    let addr = format!("{}:{}", local_lan_ip(), server.port());

    // 1. no token -> 401 (health, rpc AND events)
    let (code, body) = tls_request(&addr, &s.cert, None, &health_req()).unwrap();
    assert_eq!(code, 401, "no token: {}", body);
    let (code, _) = tls_request(
        &addr,
        &s.cert,
        None,
        &rpc_req(r#"{"jsonrpc":"2.0","id":1,"method":"daemon.ping","params":{}}"#),
    )
    .unwrap();
    assert_eq!(code, 401);
    let (code, _) = tls_request(&addr, &s.cert, None, &events_req()).unwrap();
    assert_eq!(code, 401, "events requires the token too");

    // 2. wrong token -> 401
    let wrong = "f".repeat(64);
    let (code, _) = tls_request(&addr, &s.cert, Some(&wrong), &health_req()).unwrap();
    assert_eq!(code, 401);

    // 3. correct token -> 200 + the endpoints work
    let (code, body) = tls_request(&addr, &s.cert, Some(&s.token), &health_req()).unwrap();
    assert_eq!(code, 200, "{}", body);
    assert!(body.contains("{\"ok\":true}"));
    let (code, body) = tls_request(
        &addr,
        &s.cert,
        Some(&s.token),
        &rpc_req(r#"{"jsonrpc":"2.0","id":1,"method":"downloads.list","params":{}}"#),
    )
    .unwrap();
    assert_eq!(code, 200);
    assert!(body.contains("\"result\""), "rpc works over TLS: {}", body);

    // 4. SSE over TLS with the token: head arrives, content-type correct
    let (code, body) = tls_request(&addr, &s.cert, Some(&s.token), &events_req()).unwrap();
    assert_eq!(code, 200);
    assert!(body.contains("text/event-stream"), "{}", body);

    // 5. loopback peer through the SAME wildcard listener: plain HTTP,
    //    unauthenticated, byte-identical
    let text = plain_loopback_health(server.port());
    assert!(text.starts_with("HTTP/1.1 200"), "loopback plain: {}", text);
    assert!(text.contains("{\"ok\":true}"));

    clear_secrets_env(&dir);
}

#[test]
fn plain_http_bytes_to_the_tls_path_are_refused() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = fresh_secrets_env("plain");
    let engine = test_engine();
    let server = RpcServer::start_ex(engine, "0.0.0.0:0", false).unwrap();
    let addr = format!("{}:{}", local_lan_ip(), server.port());
    let mut plain = TcpStream::connect(&addr).unwrap();
    plain
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    plain
        .write_all(b"GET /health HTTP/1.1\r\nHost: 192.168.1.5\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    let _ = plain.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    assert!(
        !text.starts_with("HTTP/1.1 200"),
        "plain HTTP must never reach the endpoints from a LAN peer: {}",
        text
    );
    assert!(!text.contains("{\"ok\":true}"));
    clear_secrets_env(&dir);
}

#[test]
fn lan_connection_budget_bounds_non_loopback_peers_only() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = fresh_secrets_env("cap");
    let engine = test_engine();
    let server = RpcServer::start_ex(engine, "0.0.0.0:0", false).unwrap();
    let s = secrets::load_or_generate(false, false).unwrap();
    let addr = format!("{}:{}", local_lan_ip(), server.port());

    // 32 idle non-loopback connections hold the full budget
    let mut idle = Vec::new();
    for _ in 0..32 {
        idle.push(TcpStream::connect(&addr).expect("idle connect"));
    }
    std::thread::sleep(Duration::from_millis(400));

    // the 33rd LAN connection is refused at accept (immediate close)
    let mut extra = TcpStream::connect(&addr).expect("tcp connect still ok");
    extra
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut buf = [0u8; 16];
    let read = extra.read(&mut buf);
    assert!(
        matches!(read, Ok(0) | Err(_)),
        "budget-exceeding peer must be closed: {:?}",
        read
    );

    // loopback peers are exempt from the budget (byte-identical path)
    let text = plain_loopback_health(server.port());
    assert!(text.starts_with("HTTP/1.1 200"));

    // freeing the slots lets a proper TLS+token request through again
    drop(idle);
    std::thread::sleep(Duration::from_millis(400));
    let (code, body) = tls_request(&addr, &s.cert, Some(&s.token), &health_req())
        .expect("request after slots freed");
    assert_eq!(code, 200, "{}", body);

    clear_secrets_env(&dir);
}

#[test]
fn lan_header_flood_is_capped_for_non_loopback_peers() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = fresh_secrets_env("hdr");
    let engine = test_engine();
    let server = RpcServer::start_ex(engine, "0.0.0.0:0", false).unwrap();
    let s = secrets::load_or_generate(false, false).unwrap();
    let addr = format!("{}:{}", local_lan_ip(), server.port());
    let mut req = String::from("GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\n");
    for i in 0..200 {
        req.push_str(&format!("X-Flood-{}: {}\r\n", i, "p".repeat(40)));
    }
    req.push_str("{AUTH}Connection: close\r\n\r\n");
    let (code, body) = tls_request(&addr, &s.cert, Some(&s.token), &req).unwrap();
    assert_eq!(code, 431, "header flood capped: {}", body);
    clear_secrets_env(&dir);
}

#[test]
fn lan_secrets_identity_is_stable_across_restarts() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = fresh_secrets_env("idem");
    let engine = test_engine();
    let server1 = RpcServer::start_ex(engine, "0.0.0.0:0", false).unwrap();
    let s1 = secrets::load_or_generate(false, false).unwrap();
    drop(server1);
    std::thread::sleep(Duration::from_millis(100));
    let engine2 = test_engine();
    let _server2 = RpcServer::start_ex(engine2, "0.0.0.0:0", false).unwrap();
    let s2 = secrets::load_or_generate(false, false).unwrap();
    assert_eq!(s1.token, s2.token, "token stable across restarts");
    assert_eq!(s1.cert, s2.cert, "cert (TOFU identity) stable");
    assert!(!s1.fingerprint.is_empty(), "fingerprint recorded");
    assert_eq!(s1.fingerprint, s2.fingerprint, "fingerprint stable");
    clear_secrets_env(&dir);
}
