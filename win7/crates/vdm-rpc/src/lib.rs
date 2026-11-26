//! JSON-RPC 2.0 over HTTP on 127.0.0.1:36990 + SSE event stream.
//!
//! Endpoints (loopback default, like IDM/XDM browser integration):
//! - `POST /rpc` — JSON-RPC 2.0 request (single or batch-lite)
//! - `GET  /events` — `text/event-stream` of engine events + progress ticks
//! - `GET  /health` — liveness probe
//!
//! The browser native-messaging host (velocitydm-nmh) and the CLI both speak
//! `POST /rpc`. Progress is pushed at 10 Hz max (GUI/extension refresh budget).
//!
//! LAN mode (S8-W1, `lan` feature): a NON-loopback bind (wildcard or explicit
//! LAN IP) is fail-closed — without the feature the server refuses to start;
//! with it, every PEER is classified at accept: loopback peers keep today's
//! byte-identical plain-HTTP path (CLI/NMH/GUI/frontends), non-loopback peers
//! must speak TLS (rustls, self-signed cert from `secrets`) and present the
//! bearer token on every endpoint. See `docs/SPECS/S8-LAN-RPC-WAVE1.md`.

pub mod secrets;

#[cfg(feature = "lan")]
pub mod lan;

use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vdm_core::bt::{BtSpec, Magnet};
use vdm_core::engine::events::Event;
use vdm_core::engine::{Engine, HttpOptions};
use vdm_core::proxy::ProxyConfig;

/// Non-loopback connection budget (spec rule 4: loopback is exempt and
/// unbounded — today's behavior).
const LAN_MAX_CONNS: usize = 32;
/// Header-count / total-size caps for non-loopback peers (loopback keeps
/// the byte-identical single-line cap only).
const LAN_HEADER_MAX_LINES: usize = 128;
const LAN_HEADER_MAX_BYTES: usize = 64 * 1024;
/// Read timeout for non-loopback peers until their first authorized
/// request completes (then the normal 300 s applies).
const LAN_PREAUTH_TIMEOUT: Duration = Duration::from_secs(10);

#[allow(dead_code)]
pub struct RpcServer {
    pub addr: std::net::SocketAddr,
    engine: Arc<Engine>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcServer {
    /// Bind and serve (loopback semantics: see [`RpcServer::start_ex`]).
    pub fn start(engine: Arc<Engine>, bind: &str) -> std::io::Result<Arc<RpcServer>> {
        RpcServer::start_ex(engine, bind, false)
    }

    /// Bind and serve; portable selects the exe-dir secrets/config layout
    /// for LAN mode (loopback binds never touch secrets).
    ///
    /// **S8-W1 fail-closed posture:** a non-loopback bind (wildcard or
    /// explicit LAN IP) REFUSES to start unless the `lan` feature is
    /// compiled in (bearer token + TLS); with it, secrets are loaded or
    /// generated and peers are demuxed per connection (loopback = plain
    /// HTTP byte-identical to the pre-LAN server; non-loopback = TLS +
    /// bearer token). Loopback binds behave exactly as before.
    pub fn start_ex(
        engine: Arc<Engine>,
        bind: &str,
        #[allow(unused_variables)] portable: bool,
    ) -> std::io::Result<Arc<RpcServer>> {
        let bind_addr = bind
            .to_socket_addrs()
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("bad bind address '{}': {}", bind, e),
                )
            })?
            .next()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty bind address")
            })?;
        let lan_mode = !bind_addr.ip().is_loopback();
        #[cfg(feature = "lan")]
        let (lan_token, lan_tls): (Option<String>, Option<Arc<rustls::ServerConfig>>) = if lan_mode
        {
            let secrets = secrets::load_or_generate(portable, true).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("LAN secrets: {}", e),
                )
            })?;
            let tls = lan::server_config(&secrets.cert, &secrets.key).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("LAN tls: {}", e))
            })?;
            (Some(secrets.token), Some(Arc::new(tls)))
        } else {
            (None, None)
        };
        #[cfg(not(feature = "lan"))]
        if lan_mode {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "LAN bind refused: non-loopback binds require the `lan` feature \
                 (bearer token + TLS); rebuild with --features lan",
            ));
        }
        #[cfg(not(feature = "lan"))]
        let (lan_token, lan_tls): (Option<String>, Option<()>) = (None, None);
        let listener = TcpListener::bind(bind)?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let engine_c = engine.clone();
        let stop_c = stop.clone();
        let lan_conns = Arc::new(AtomicUsize::new(0));
        listener.set_nonblocking(true)?;
        let handle = std::thread::spawn(move || {
            let _ = &lan_tls; // capture on both feature states
            loop {
                if stop_c.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        // macOS/BSD/Windows: accepted sockets inherit the
                        // listener's non-blocking flag (Linux does not); the RPC
                        // connection loop is blocking I/O.
                        let _ = stream.set_nonblocking(false);
                        // S8-W1 peer classification: fail closed on
                        // unresolvable peers; IPv4-mapped v6 loopback is
                        // canonicalized (see `peer_is_loopback`).
                        let peer = match stream.peer_addr() {
                            Ok(p) => p,
                            Err(_) => continue,
                        };
                        let peer_lan = !peer_is_loopback(&peer);
                        let guard = if peer_lan {
                            if lan_conns.fetch_add(1, Ordering::Relaxed) >= LAN_MAX_CONNS {
                                lan_conns.fetch_sub(1, Ordering::Relaxed);
                                continue;
                            }
                            Some(ConnGuard::new(lan_conns.clone()))
                        } else {
                            None
                        };
                        let conn = if !peer_lan {
                            Conn::Plain(stream)
                        } else {
                            #[cfg(feature = "lan")]
                            {
                                let cfg = lan_tls
                                    .as_ref()
                                    .expect("lan bind guarantees a tls config")
                                    .clone();
                                Conn::Tls(Box::new(rustls::StreamOwned::new(
                                    rustls::ServerConnection::new(cfg)
                                        .expect("valid server config"),
                                    stream,
                                )))
                            }
                            #[cfg(not(feature = "lan"))]
                            {
                                let _ = stream;
                                unreachable!("non-loopback bind refused at start_ex")
                            }
                        };
                        let engine = engine_c.clone();
                        let stop = stop_c.clone();
                        let bind = addr;
                        let token = lan_token.clone();
                        std::thread::spawn(move || {
                            let _ = handle_conn(
                                SharedConn::new(conn),
                                engine,
                                stop,
                                bind,
                                peer_lan,
                                token,
                                guard,
                            );
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Arc::new(RpcServer {
            addr,
            engine,
            stop,
            handle: Some(handle),
        }))
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Whether a shutdown has been requested (via [`RpcServer::stop`] or the
    /// `daemon.shutdown` RPC): the accept loop and SSE loops are winding down.
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// Whether a request's `Host` header may reach this server.
///
/// The authority's host part must be a loopback name (`localhost`,
/// 127.0.0.0/8, ::1) when the server binds loopback — any hostname of our
/// own would defeat DNS-rebinding protection, and a rebound attacker domain
/// can never appear here as an IP literal. For wildcard binds (explicit
/// LAN mode) any IP literal or `localhost` is accepted (names other than
/// localhost are still refused). Otherwise the host must equal the bound
/// host. The port is intentionally not checked: it is not a rebinding
/// vector.
fn host_allowed(host_hdr: &str, bind: std::net::SocketAddr) -> bool {
    let authority = host_hdr.split('/').next().unwrap_or("").trim();
    let authority = match authority.rsplit_once('@') {
        Some((_, a)) => a,
        None => authority,
    };
    let host_part = match authority.rsplit_once(':') {
        Some((h, _)) => h,
        None => authority,
    };
    let host_part = host_part.trim_start_matches('[').trim_end_matches(']');
    let host_lc = host_part.to_ascii_lowercase();
    let is_ip = host_lc.parse::<std::net::IpAddr>().is_ok();
    let is_loopback_ip = host_lc
        .parse::<std::net::IpAddr>()
        .map(|i| i.is_loopback())
        .unwrap_or(false);
    let is_localhost = host_lc == "localhost";
    match bind.ip() {
        std::net::IpAddr::V4(v4) if v4.is_loopback() => is_localhost || is_loopback_ip,
        std::net::IpAddr::V6(v6) if v6.is_loopback() => is_localhost || is_loopback_ip,
        std::net::IpAddr::V4(v4) if v4.is_unspecified() => is_localhost || is_ip,
        std::net::IpAddr::V6(v6) if v6.is_unspecified() => is_localhost || is_ip,
        bound => host_lc == bound.to_string(),
    }
}

/// Whether a bind address string names a loopback authority (the
/// daemon uses this to decide whether to print the LAN token after a
/// successful `start_ex`).
pub fn bind_is_loopback(bind: &str) -> bool {
    bind.to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .map(|a| a.ip().is_loopback())
        .unwrap_or(false) // unresolvable binds fail inside start_ex anyway
}

/// Loopback peer classification with IPv4-mapped canonicalization: a
/// v6 wildcard bind presents v4 loopback peers as `::ffff:127.0.0.1`,
/// whose `is_loopback()` is false — canonicalize before checking
/// (gate 0018, ATKR condition).
fn peer_is_loopback(addr: &std::net::SocketAddr) -> bool {
    match addr.ip() {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.is_loopback(),
            None => v6.is_loopback(),
        },
    }
}

/// One accepted connection: plain TCP (loopback peers, byte-identical
/// path) or a rustls server stream (non-loopback peers, `lan` feature).
enum Conn {
    Plain(TcpStream),
    // Boxed: StreamOwned dwarfs TcpStream (clippy large_enum_variant);
    // one heap alloc per LAN connection is noise.
    #[cfg(feature = "lan")]
    Tls(Box<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>),
}

impl Conn {
    fn set_read_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
        match self {
            Conn::Plain(s) => s.set_read_timeout(d),
            #[cfg(feature = "lan")]
            Conn::Tls(s) => s.sock.set_read_timeout(d),
        }
    }
    fn set_write_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
        match self {
            Conn::Plain(s) => s.set_write_timeout(d),
            #[cfg(feature = "lan")]
            Conn::Tls(s) => s.sock.set_write_timeout(d),
        }
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            #[cfg(feature = "lan")]
            Conn::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(buf),
            #[cfg(feature = "lan")]
            Conn::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            #[cfg(feature = "lan")]
            Conn::Tls(s) => s.flush(),
        }
    }
}

/// Reader/writer split over any connection (the plain path previously
/// used `TcpStream::try_clone`; rustls streams cannot, so both halves
/// share one mutex — RPC request rates make the lock cost irrelevant).
struct SharedConn<S> {
    inner: Arc<Mutex<S>>,
}

impl<S> Clone for SharedConn<S> {
    fn clone(&self) -> Self {
        SharedConn {
            inner: self.inner.clone(),
        }
    }
}

impl<S> SharedConn<S> {
    fn new(s: S) -> Self {
        SharedConn {
            inner: Arc::new(Mutex::new(s)),
        }
    }
}

impl<S: Read> Read for SharedConn<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.lock().unwrap().read(buf)
    }
}

impl<S: Write> Write for SharedConn<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.lock().unwrap().flush()
    }
}

impl SharedConn<Conn> {
    fn set_read_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
        self.inner.lock().unwrap().set_read_timeout(d)
    }
    fn set_write_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
        self.inner.lock().unwrap().set_write_timeout(d)
    }
}

/// Decrements the non-loopback connection budget when the connection
/// thread ends (held for the connection's lifetime).
struct ConnGuard {
    counter: Arc<AtomicUsize>,
}

impl ConnGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        ConnGuard { counter }
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

fn handle_conn(
    mut conn: SharedConn<Conn>,
    engine: Arc<Engine>,
    stop: Arc<AtomicBool>,
    bind: std::net::SocketAddr,
    peer_lan: bool,
    token: Option<String>,
    guard: Option<ConnGuard>,
) -> std::io::Result<()> {
    let _ = &guard; // held for the connection lifetime
    let _ = conn.set_read_timeout(Some(if peer_lan {
        LAN_PREAUTH_TIMEOUT
    } else {
        Duration::from_secs(300)
    }));
    let _ = conn.set_write_timeout(Some(Duration::from_secs(30)));
    let mut reader = std::io::BufReader::new(conn.clone());
    // one HTTP request per connection (keep-alive supported, bounded)
    for _ in 0..256 {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line)? == 0 {
            return Ok(());
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("/").to_string();
        let mut content_length = 0usize;
        let mut keep_alive = true;
        let mut host: Option<String> = None;
        let mut origin_seen = false;
        let mut referer_seen = false;
        let mut content_type: Option<String> = None;
        let mut authorization: Option<String> = None;
        let mut header_lines = 0usize;
        let mut header_bytes = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                return Ok(());
            }
            if line.len() > 16 * 1024 {
                write_response(&mut conn, 431, "text/plain", b"header too large", false)?;
                return Ok(());
            }
            // S8-W1: non-loopback peers get a bounded header block
            // (count + total size) so a dribbling header flood cannot
            // pin a connection slot for the full read timeout; loopback
            // keeps the byte-identical single-line cap only.
            if peer_lan {
                header_lines += 1;
                header_bytes += line.len();
                if header_lines > LAN_HEADER_MAX_LINES || header_bytes > LAN_HEADER_MAX_BYTES {
                    write_response(&mut conn, 431, "text/plain", b"too many headers", false)?;
                    return Ok(());
                }
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some((k, v)) = trimmed.split_once(':') {
                let k = k.trim();
                let v = v.trim();
                if k.eq_ignore_ascii_case("content-length") {
                    content_length = v.parse().unwrap_or(0);
                }
                if k.eq_ignore_ascii_case("connection") {
                    keep_alive = v.eq_ignore_ascii_case("keep-alive");
                }
                if k.eq_ignore_ascii_case("host") {
                    host = Some(v.to_string());
                }
                if k.eq_ignore_ascii_case("origin") {
                    origin_seen = true;
                }
                if k.eq_ignore_ascii_case("referer") {
                    referer_seen = true;
                }
                if k.eq_ignore_ascii_case("content-type") {
                    content_type = Some(v.to_string());
                }
                if k.eq_ignore_ascii_case("authorization") {
                    authorization = Some(v.to_string());
                }
            }
        }
        // --- Loopback trust boundary ---------------------------------------
        // The RPC is unauthenticated by design (local CLI / native-messaging
        // host / in-process GUI). Three rules keep web pages out:
        //   1. Host must be a loopback name (or the bound host) — kills DNS
        //      rebinding, which needs the attacker's own hostname in Host.
        //   2. Origin/Referer must be absent — browsers always send one on
        //      cross-site fetch/sendBeacon/form POSTs; native clients never.
        //   3. POST /rpc must be application/json — defeats no-cors `simple`
        //      requests (text/plain form posts) that execute without
        //      preflight.
        let host_ok = match host.as_deref() {
            Some(h) => host_allowed(h, bind),
            None => false, // HTTP/1.1 requires Host; HTTP/1.0 is not a client we serve
        };
        if !host_ok || origin_seen || referer_seen {
            write_response(&mut conn, 403, "text/plain", b"forbidden", false)?;
            return Ok(());
        }
        // --- LAN trust boundary (S8-W1) -----------------------------------
        // Non-loopback peers must present the bearer token on EVERY
        // endpoint (checked after the header block, before the body —
        // spec rule 2/4). Loopback peers never reach this branch, so
        // their path stays byte-identical.
        if peer_lan {
            let expected = token.as_deref().unwrap_or("");
            let presented = authorization
                .as_deref()
                .and_then(bearer_token)
                .unwrap_or("");
            if !secrets::token_matches(presented, expected) {
                write_response(&mut conn, 401, "text/plain", b"unauthorized", false)?;
                return Ok(());
            }
            // first authorized request: adopt the normal read timeout
            let _ = conn.set_read_timeout(Some(Duration::from_secs(300)));
        }
        if method == "POST" {
            let ct_ok = content_type
                .as_deref()
                .map(|c| c.to_ascii_lowercase().starts_with("application/json"))
                .unwrap_or(false);
            if !ct_ok {
                write_response(
                    &mut conn,
                    415,
                    "text/plain",
                    b"content-type must be application/json",
                    false,
                )?;
                return Ok(());
            }
        }
        if content_length > 8 * 1024 * 1024 {
            write_response(
                &mut conn,
                413,
                "text/plain",
                b"payload too large",
                keep_alive,
            )?;
            return Ok(());
        }
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body)?;

        match (method.as_str(), path.as_str()) {
            ("GET", "/health") => {
                write_response(
                    &mut conn,
                    200,
                    "application/json",
                    b"{\"ok\":true}",
                    keep_alive,
                )?;
            }
            ("POST", "/rpc") => {
                let resp = dispatch(&engine, &body, &stop);
                let payload = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{}".to_vec());
                write_response(&mut conn, 200, "application/json", &payload, keep_alive)?;
            }
            ("GET", "/events") => {
                sse_loop(&mut conn, &engine, &stop)?;
                return Ok(());
            }
            _ => {
                write_response(&mut conn, 404, "text/plain", b"not found", keep_alive)?;
            }
        }
        if !keep_alive {
            return Ok(());
        }
    }
    Ok(())
}

/// Extract the token from an `Authorization` header value (scheme is
/// case-insensitive per RFC 9110; anything else is not a bearer token).
fn bearer_token(header_value: &str) -> Option<&str> {
    let v = header_value.trim();
    if v.len() > 7 && v[..7].eq_ignore_ascii_case("bearer ") {
        Some(v[7..].trim())
    } else {
        None
    }
}

fn write_response<W: Write>(
    stream: &mut W,
    status: u16,
    ctype: &str,
    body: &[u8],
    keep_alive: bool,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let conn = if keep_alive { "keep-alive" } else { "close" };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n",
        status,
        reason,
        ctype,
        body.len(),
        conn
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// SSE: engine events + periodic snapshot deltas (10 Hz).
fn sse_loop<W: Write>(
    stream: &mut W,
    engine: &Arc<Engine>,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    stream.write_all(head.as_bytes())?;
    stream.flush()?;
    let rx = engine.subscribe();
    let mut last_tick = std::time::Instant::now();
    let mut last_state = std::collections::HashMap::<u64, String>::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // engine events first (non-blocking)
        while let Ok(evt) = rx.try_recv() {
            let payload = event_payload(&evt);
            send_sse(stream, &payload)?;
        }
        // 10 Hz progress ticks with change detection
        if last_tick.elapsed() >= Duration::from_millis(100) {
            last_tick = std::time::Instant::now();
            let snaps = engine.list();
            let mut changed = Vec::new();
            for s in &snaps {
                let key = format!(
                    "{}|{}|{}|{}",
                    s.state.as_str(),
                    s.done,
                    s.speed,
                    s.error.is_some()
                );
                if last_state.get(&s.id) != Some(&key) {
                    last_state.insert(s.id, key);
                    changed.push(s.clone());
                }
            }
            if !changed.is_empty() {
                let payload = json!({ "type": "tasks", "tasks": changed });
                send_sse(stream, &payload)?;
            }
        }
        // detect client disconnect: a zero-length write fails or a poll write
        if stream.write_all(b"").is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn send_sse<W: Write>(stream: &mut W, payload: &Value) -> std::io::Result<()> {
    let text = payload.to_string();
    let frame = format!("data: {}\n\n", text.replace('\n', " "));
    stream.write_all(frame.as_bytes())?;
    stream.flush()
}

fn event_payload(evt: &Event) -> Value {
    match evt {
        Event::TaskAdded { id } => json!({ "type": "task-added", "id": id }),
        Event::TaskStateChanged { id, state } => {
            json!({ "type": "task-state", "id": id, "state": state.as_str() })
        }
        Event::TaskProgress { id } => json!({ "type": "task-progress", "id": id }),
        Event::TaskCompleted { id, path } => {
            json!({ "type": "task-completed", "id": id, "path": path })
        }
        Event::TaskFailed { id, error } => {
            json!({ "type": "task-failed", "id": id, "error": error })
        }
        Event::TaskRemoved { id } => json!({ "type": "task-removed", "id": id }),
        Event::QueueDrained => json!({ "type": "queue-drained" }),
        Event::ConfigChanged => json!({ "type": "config-changed" }),
    }
}

/// JSON-RPC 2.0 dispatch.
///
/// `stop` is the owning server's shutdown flag; the `daemon.shutdown`
/// method stops the engine AND requests the server wind-down so the daemon
/// process exits cleanly (the daemon's main loop watches the server state).
pub fn dispatch(engine: &Arc<Engine>, body: &[u8], stop: &AtomicBool) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(Value::Null, -32700, &format!("parse error: {}", e));
        }
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "daemon.ping" => ok_response(&id, json!("pong")),
        "daemon.info" => ok_response(
            &id,
            json!({
                "name": "VelocityDM",
                "version": vdm_core::VERSION,
                "rpc": "json-rpc-2.0",
                "features": ["http", "hls", "bittorrent", "magnet", "batch", "scheduler", "proxy", "refresh"]
            }),
        ),
        "daemon.shutdown" => {
            engine.shutdown();
            stop.store(true, Ordering::Relaxed);
            ok_response(&id, json!(true))
        }
        "downloads.list" => {
            let snaps = engine.list();
            ok_response(&id, json!(snaps))
        }
        "downloads.get" => {
            let want = params.get("id").and_then(|v| v.as_u64());
            match want {
                Some(want) => match engine.get(want) {
                    Some(s) => ok_response(&id, json!(s)),
                    None => error_response(id, -32001, "no such task"),
                },
                None => error_response(id, -32002, "missing id"),
            }
        }
        "downloads.add" => {
            let url = match params.get("url").and_then(|v| v.as_str()) {
                Some(u) => u.to_string(),
                None => return error_response(id, -32002, "missing url"),
            };
            let opts: HttpOptions =
                serde_json::from_value(params.get("options").cloned().unwrap_or(json!({})))
                    .unwrap_or_default();
            match engine.add_download(&url, opts) {
                Ok(task_id) => {
                    // S11-i B16: an add-time warning (e.g. the task proxy
                    // differs from the browser's proxy) rides the reply
                    let mut reply = json!({ "id": task_id });
                    if let Some(notice) = engine.get(task_id).and_then(|s| s.notice) {
                        reply["notice"] = json!(notice);
                    }
                    ok_response(&id, reply)
                }
                Err(e) => error_response(id, -32003, &e.to_string()),
            }
        }
        "downloads.addBatch" => {
            let pattern = match params.get("pattern").and_then(|v| v.as_str()) {
                Some(u) => u.to_string(),
                None => return error_response(id, -32002, "missing pattern"),
            };
            let opts: HttpOptions =
                serde_json::from_value(params.get("options").cloned().unwrap_or(json!({})))
                    .unwrap_or_default();
            match engine.add_batch(&pattern, opts) {
                Ok(ids) => ok_response(&id, json!({ "ids": ids })),
                Err(e) => error_response(id, -32003, &e.to_string()),
            }
        }
        "batch.expand" => match vdm_core::batch::expand_pattern(
            params.get("pattern").and_then(|v| v.as_str()).unwrap_or(""),
        ) {
            Ok(urls) => ok_response(&id, json!({ "urls": urls })),
            Err(e) => error_response(id, -32004, &e.to_string()),
        },
        "downloads.addTorrent" => {
            let spec = match torrent_spec_from_params(&params) {
                Ok(s) => s,
                Err(msg) => return error_response(id, -32002, &msg),
            };
            let out_dir = params
                .get("outDir")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let priorities: Vec<u8> = params
                .get("filePriorities")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().map(|x| x.as_u64().unwrap_or(1) as u8).collect())
                .unwrap_or_default();
            let limit = params
                .get("rateLimitBps")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            match engine.add_torrent(spec, out_dir, priorities, limit) {
                Ok(task_id) => ok_response(&id, json!({ "id": task_id })),
                Err(e) => error_response(id, -32003, &e.to_string()),
            }
        }
        "torrents.files" => {
            // list files of a .torrent (pre-add inspection, Motrix-style)
            let spec = match torrent_spec_from_params(&params) {
                Ok(s) => s,
                Err(msg) => return error_response(id, -32002, &msg),
            };
            match spec {
                BtSpec::TorrentFile(path) => match std::fs::read(&path)
                    .map_err(|e| vdm_core::Error::Torrent(format!("{}", e)))
                    .and_then(|b| vdm_core::bt::Metainfo::from_torrent_bytes(&b))
                {
                    Ok(meta) => ok_response(
                        &id,
                        json!({
                            "name": meta.name,
                            "totalLength": meta.total_len,
                            "infoHash": hex::encode(meta.info_hash),
                            "trackers": meta.trackers,
                            "files": meta.files.iter().map(|f| json!({
                                "path": f.path.join("/"),
                                "length": f.length,
                            })).collect::<Vec<_>>()
                        }),
                    ),
                    Err(e) => error_response(id, -32003, &e.to_string()),
                },
                BtSpec::TorrentBytes(b) => match vdm_core::bt::Metainfo::from_torrent_bytes(&b) {
                    Ok(meta) => ok_response(
                        &id,
                        json!({
                            "name": meta.name,
                            "totalLength": meta.total_len,
                            "infoHash": hex::encode(meta.info_hash),
                            "trackers": meta.trackers,
                            "files": meta.files.iter().map(|f| json!({
                                "path": f.path.join("/"),
                                "length": f.length,
                            })).collect::<Vec<_>>()
                        }),
                    ),
                    Err(e) => error_response(id, -32003, &e.to_string()),
                },
                BtSpec::Magnet(_) => {
                    error_response(id, -32005, "magnet has no file list before metadata")
                }
            }
        }
        "downloads.pause" => match param_id(&params).and_then(|i| engine.pause(i).ok()) {
            Some(()) => ok_response(&id, json!(true)),
            None => error_response(id, -32001, "no such task"),
        },
        "downloads.pauseAll" => {
            engine.pause_all();
            ok_response(&id, json!(true))
        }
        "downloads.resume" => match param_id(&params).and_then(|i| engine.resume(i).ok()) {
            Some(()) => ok_response(&id, json!(true)),
            None => error_response(id, -32001, "no such task"),
        },
        "downloads.remove" => {
            let delete = params
                .get("deleteFiles")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match param_id(&params).and_then(|i| engine.remove(i, delete).ok()) {
                Some(()) => ok_response(&id, json!(true)),
                None => error_response(id, -32001, "no such task"),
            }
        }
        "downloads.refresh" => {
            let new_url = match params.get("url").and_then(|v| v.as_str()) {
                Some(u) => u.to_string(),
                None => return error_response(id, -32002, "missing url"),
            };
            match param_id(&params).and_then(|i| engine.refresh(i, &new_url).ok()) {
                Some(()) => ok_response(&id, json!(true)),
                None => error_response(id, -32006, "refresh failed (pause the task first?)"),
            }
        }
        "downloads.priority" => {
            let p = params.get("priority").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            match param_id(&params).and_then(|i| engine.set_priority(i, p).ok()) {
                Some(()) => ok_response(&id, json!(true)),
                None => error_response(id, -32001, "no such task"),
            }
        }
        "queue.get" => {
            let cfg = engine.config();
            ok_response(
                &id,
                json!({
                    "maxActive": cfg.scheduler.max_active,
                    "stopWindow": cfg.scheduler.stop_window,
                    "shutdownWhenDone": cfg.scheduler.shutdown_when_done,
                    "onQueueDrained": cfg.scheduler.on_queue_drained,
                }),
            )
        }
        "queue.set" => {
            let mut cfg = engine.config();
            if let Some(v) = params.get("maxActive").and_then(|v| v.as_u64()) {
                cfg.scheduler.max_active = v.max(1) as usize;
            }
            if let Some(v) = params.get("shutdownWhenDone").and_then(|v| v.as_bool()) {
                cfg.scheduler.shutdown_when_done = v;
            }
            if let Some(v) = params.get("onQueueDrained").and_then(|v| v.as_str()) {
                cfg.scheduler.on_queue_drained = Some(v.to_string());
            }
            if let Some(arr) = params.get("stopWindow").and_then(|v| v.as_array()) {
                if arr.len() == 2 {
                    cfg.scheduler.stop_window = Some((
                        arr[0].as_str().unwrap_or("00:00").to_string(),
                        arr[1].as_str().unwrap_or("00:00").to_string(),
                    ));
                } else if arr.is_empty() {
                    cfg.scheduler.stop_window = None;
                }
            }
            engine.set_config(cfg);
            ok_response(&id, json!(true))
        }
        "limits.set" => {
            if let Some(v) = params.get("globalBps").and_then(|v| v.as_u64()) {
                engine.set_global_limit(v);
                ok_response(&id, json!(true))
            } else {
                error_response(id, -32002, "missing globalBps")
            }
        }
        "limits.get" => ok_response(
            &id,
            json!({ "globalBps": engine.config().global_limit_bps }),
        ),
        "settings.get" => {
            let cfg = engine.config();
            ok_response(
                &id,
                json!({
                    "downloadDir": cfg.download_dir.to_string_lossy(),
                    "defaultConnections": cfg.default_connections,
                    "userAgent": cfg.user_agent,
                    "globalLimitBps": cfg.global_limit_bps,
                    "proxy": cfg.proxy,
                    // S11-i B22: TLS minimum-version policy (default|tls12|tls13)
                    "tlsMin": cfg.tls_min,
                    "scheduler": cfg.scheduler,
                }),
            )
        }
        "settings.set" => {
            let mut cfg = engine.config();
            if let Some(v) = params.get("downloadDir").and_then(|v| v.as_str()) {
                cfg.download_dir = std::path::PathBuf::from(v);
            }
            if let Some(v) = params.get("defaultConnections").and_then(|v| v.as_u64()) {
                cfg.default_connections = v.min(32) as usize;
            }
            if let Some(v) = params.get("userAgent").and_then(|v| v.as_str()) {
                cfg.user_agent = v.to_string();
            }
            if let Some(v) = params.get("globalLimitBps").and_then(|v| v.as_u64()) {
                cfg.global_limit_bps = v;
            }
            if params.get("proxy").map(|v| v.is_null()).unwrap_or(false) {
                cfg.proxy = None;
            } else if let Some(v) = params.get("proxy") {
                if let Ok(p) = serde_json::from_value::<ProxyConfig>(v.clone()) {
                    cfg.proxy = Some(p);
                }
            }
            // S11-i B22: validated set path — unknown tlsMin values are
            // rejected here (the tolerant load path is only for config.json
            // files written by older/other builds).
            if let Some(v) = params.get("tlsMin") {
                match v.as_str().and_then(vdm_core::tls::TlsMin::parse) {
                    Some(t) => cfg.tls_min = t.as_str().to_string(),
                    None => {
                        return error_response(
                            id,
                            -32602,
                            "tlsMin must be one of: default, tls12, tls13",
                        )
                    }
                }
            }
            engine.set_config(cfg);
            ok_response(&id, json!(true))
        }
        _ => error_response(id, -32601, &format!("unknown method '{}'", method)),
    }
}

fn param_id(params: &Value) -> Option<u64> {
    params.get("id").and_then(|v| v.as_u64())
}

fn torrent_spec_from_params(params: &Value) -> Result<BtSpec, String> {
    if let Some(path) = params.get("torrentPath").and_then(|v| v.as_str()) {
        return Ok(BtSpec::TorrentFile(std::path::PathBuf::from(path)));
    }
    if let Some(mag) = params.get("magnet").and_then(|v| v.as_str()) {
        return Magnet::parse(mag)
            .map(BtSpec::Magnet)
            .map_err(|e| e.to_string());
    }
    Err("missing torrentPath or magnet".to_string())
}

fn ok_response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// ---------------------------------------------------------------------------
// Client side (shared by CLI, NMH, tests)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RpcClient {
    pub addr: String,
    timeout: Duration,
}

impl Default for RpcClient {
    fn default() -> Self {
        RpcClient {
            addr: vdm_core::RPC_ADDR.to_string(),
            timeout: Duration::from_secs(60),
        }
    }
}

impl RpcClient {
    pub fn new(addr: &str) -> RpcClient {
        RpcClient {
            addr: addr.to_string(),
            timeout: Duration::from_secs(60),
        }
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, vdm_core::Error> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let req = format!(
            "POST /rpc HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.addr,
            body.to_string().len()
        );
        let mut stream = TcpStream::connect(&self.addr)
            .map_err(|e| vdm_core::Error::Http(format!("connect daemon: {}", e)))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .and_then(|_| stream.set_write_timeout(Some(self.timeout)))?;
        stream
            .write_all(req.as_bytes())
            .and_then(|_| stream.write_all(body.to_string().as_bytes()))
            .and_then(|_| stream.flush())
            .map_err(vdm_core::Error::Io)?;
        // read head + body
        let mut raw = Vec::new();
        let mut chunk = [0u8; 4096];
        let mut content_length: Option<usize> = None;
        let mut head_end = None;
        loop {
            let n = stream.read(&mut chunk).map_err(vdm_core::Error::Io)?;
            if n == 0 {
                break;
            }
            let prev = raw.len();
            raw.extend_from_slice(&chunk[..n]);
            if head_end.is_none() {
                if let Some(pos) = find_crlf2(&raw, prev.saturating_sub(4)) {
                    head_end = Some(pos + 4);
                    let head = String::from_utf8_lossy(&raw[..pos + 4]).to_string();
                    for line in head.lines() {
                        if let Some((k, v)) = line.split_once(':') {
                            if k.trim().eq_ignore_ascii_case("content-length") {
                                content_length = v.trim().parse().ok();
                            }
                        }
                    }
                }
            }
            if let (Some(he), Some(cl)) = (head_end, content_length) {
                if raw.len() >= he + cl {
                    break;
                }
            }
            if raw.len() > 16 * 1024 * 1024 {
                return Err(vdm_core::Error::Http("rpc response too large".into()));
            }
        }
        let he = head_end.ok_or_else(|| vdm_core::Error::Http("no rpc response head".into()))?;
        let body = &raw[he..];
        let v: Value = serde_json::from_slice(body)
            .map_err(|e| vdm_core::Error::Http(format!("bad rpc json: {}", e)))?;
        if let Some(err) = v.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("rpc error");
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            return Err(vdm_core::Error::Task(format!("rpc {}: {}", code, msg)));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn find_crlf2(buf: &[u8], from: usize) -> Option<usize> {
    let from = from.min(buf.len().saturating_sub(4));
    let mut i = from;
    while i + 4 <= buf.len() {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use vdm_core::engine::EngineConfig;

    fn start_test_engine() -> Arc<Engine> {
        let dir = std::env::temp_dir().join(format!("vdm-rpc-test-{}", std::process::id()));
        Engine::new(EngineConfig {
            download_dir: dir,
            ..Default::default()
        })
    }

    #[test]
    fn peer_loopback_classification_including_mapped_v4() {
        // S8-W1 gate 0018: ::ffff:127.0.0.1 must classify as loopback.
        assert!(peer_is_loopback(&"127.0.0.1:1".parse().unwrap()));
        assert!(peer_is_loopback(&"127.255.0.9:1".parse().unwrap()));
        assert!(peer_is_loopback(&"[::1]:1".parse().unwrap()));
        assert!(peer_is_loopback(&"[::ffff:127.0.0.1]:1".parse().unwrap()));
        assert!(!peer_is_loopback(&"192.168.1.5:1".parse().unwrap()));
        assert!(!peer_is_loopback(
            &"[::ffff:192.168.1.5]:1".parse().unwrap()
        ));
        assert!(!peer_is_loopback(&"[fe80::1]:1".parse().unwrap()));
    }

    #[test]
    fn bearer_token_extraction_is_scheme_case_insensitive() {
        assert_eq!(bearer_token("Bearer abc"), Some("abc"));
        assert_eq!(bearer_token("bearer abc"), Some("abc"));
        assert_eq!(bearer_token("  BEARER   tok  "), Some("tok"));
        assert_eq!(bearer_token("Basic abc"), None);
        assert_eq!(bearer_token(""), None);
    }

    #[test]
    #[cfg(not(feature = "lan"))]
    fn lan_bind_refused_without_the_feature() {
        // S8-W1 fail-closed posture: a wildcard bind must never serve
        // unauthenticated — without the `lan` feature the server refuses
        // to start at all (this witness compiles only in default builds;
        // the positive path lives in tests/lan.rs under the feature).
        let engine = start_test_engine();
        let err = match RpcServer::start_ex(engine, "0.0.0.0:0", false) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("wildcard bind must be refused without the lan feature"),
        };
        assert!(err.contains("lan"), "{}", err);
    }

    #[test]
    #[cfg(not(feature = "lan"))]
    fn explicit_lan_ip_bind_refused_without_the_feature() {
        let engine = start_test_engine();
        // 192.0.2.1 is TEST-NET-1: non-loopback by construction
        assert!(RpcServer::start_ex(engine, "192.0.2.1:0", false).is_err());
    }

    #[test]
    fn loopback_bind_still_starts_in_default_builds() {
        let engine = start_test_engine();
        let server = RpcServer::start_ex(engine, "127.0.0.1:0", false).unwrap();
        assert!(server.addr.ip().is_loopback());
    }

    #[test]
    fn rpc_roundtrip_and_methods() {
        let engine = start_test_engine();
        let server = RpcServer::start(engine.clone(), "127.0.0.1:0").unwrap();
        let client = RpcClient::new(&format!("127.0.0.1:{}", server.port()));

        assert_eq!(
            client.call("daemon.ping", json!({})).unwrap(),
            json!("pong")
        );
        let info = client.call("daemon.info", json!({})).unwrap();
        assert_eq!(info["name"], "VelocityDM");
        assert!(info["features"].as_array().unwrap().len() >= 6);

        // add + list + get + pause + resume + remove
        let r = client
            .call(
                "downloads.add",
                json!({ "url": "http://127.0.0.1:1/nope", "options": { "filename": "x.bin" } }),
            )
            .unwrap();
        let id = r["id"].as_u64().unwrap();
        let list = client.call("downloads.list", json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        let got = client.call("downloads.get", json!({ "id": id })).unwrap();
        assert_eq!(got["id"].as_u64().unwrap(), id);
        client.call("downloads.pause", json!({ "id": id })).unwrap();
        let paused = client.call("downloads.get", json!({ "id": id })).unwrap();
        assert!(paused["userPaused"].as_bool().unwrap());
        client
            .call("downloads.resume", json!({ "id": id }))
            .unwrap();
        client
            .call("downloads.remove", json!({ "id": id }))
            .unwrap();
        let list2 = client.call("downloads.list", json!({})).unwrap();
        assert_eq!(list2.as_array().unwrap().len(), 0);

        // batch expansion endpoint
        let b = client
            .call("batch.expand", json!({ "pattern": "http://x/p[1:2].jpg" }))
            .unwrap();
        assert_eq!(b["urls"].as_array().unwrap().len(), 2);

        // settings roundtrip
        client
            .call(
                "settings.set",
                json!({ "downloadDir": "/tmp/vdm-rpc-dir", "defaultConnections": 8 }),
            )
            .unwrap();
        let s = client.call("settings.get", json!({})).unwrap();
        assert_eq!(s["defaultConnections"].as_u64().unwrap(), 8);

        // limits
        client
            .call("limits.set", json!({ "globalBps": 102400 }))
            .unwrap();
        let l = client.call("limits.get", json!({})).unwrap();
        assert_eq!(l["globalBps"].as_u64().unwrap(), 102400);

        // queue settings
        client
            .call(
                "queue.set",
                json!({ "maxActive": 5, "stopWindow": ["22:00", "06:00"] }),
            )
            .unwrap();
        let q = client.call("queue.get", json!({})).unwrap();
        assert_eq!(q["maxActive"].as_u64().unwrap(), 5);
        assert_eq!(q["stopWindow"][0], "22:00");

        // errors
        let err = client.call("no.such.method", json!({})).unwrap_err();
        assert!(err.to_string().contains("unknown method"));
        let err2 = client
            .call("downloads.get", json!({ "id": 9999 }))
            .unwrap_err();
        assert!(err2.to_string().contains("no such task"));
        let err3 = client.call("downloads.add", json!({})).unwrap_err();
        assert!(err3.to_string().contains("missing url"));

        // health via raw http
        let mut s = std::net::TcpStream::connect(format!("127.0.0.1:{}", server.port())).unwrap();
        s.write_all(
            format!(
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                server.port()
            )
            .as_bytes(),
        )
        .unwrap();
        let mut buf = String::new();
        let _ = s.read_to_string(&mut buf);
        assert!(buf.contains("200 OK"));
        assert!(buf.contains("\"ok\":true"));

        server.stop();
        engine.shutdown();
    }

    /// S11-i B16: an add-time warning (task proxy differs from the browser's
    /// reported proxy) rides the `downloads.add` reply.
    #[test]
    fn add_reply_carries_proxy_notice() {
        let engine = start_test_engine();
        let server = RpcServer::start(engine.clone(), "127.0.0.1:0").unwrap();
        let client = RpcClient::new(&format!("127.0.0.1:{}", server.port()));
        // no proxy reported: plain reply, no notice key
        let plain = client
            .call(
                "downloads.add",
                json!({ "url": "http://127.0.0.1:1/nope", "options": { "filename": "a.bin" } }),
            )
            .unwrap();
        assert!(plain.get("notice").is_none());
        // an explicit task proxy differing from the browser's: notice rides
        let r = client
            .call(
                "downloads.add",
                json!({
                    "url": "http://127.0.0.1:1/nope",
                    "options": {
                        "filename": "b.bin",
                        "proxy": { "type": "socks5", "host": "127.0.0.1", "port": 1080 },
                        "browserProxy": { "type": "http", "host": "127.0.0.1", "port": 3128 }
                    }
                }),
            )
            .unwrap();
        assert_eq!(
            r["notice"].as_str().unwrap(),
            "task proxy differs from the browser's proxy; downloads may not follow the browser's route"
        );
        server.stop();
        engine.shutdown();
    }

    #[test]
    fn sse_stream_emits_events() {
        let engine = start_test_engine();
        let server = RpcServer::start(engine.clone(), "127.0.0.1:0").unwrap();
        let mut s = std::net::TcpStream::connect(format!("127.0.0.1:{}", server.port())).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(
            format!(
                "GET /events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                server.port()
            )
            .as_bytes(),
        )
        .unwrap();
        s.flush().unwrap();
        // wait for the SSE header so the server side has subscribed
        let mut got = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline && !got.contains("text/event-stream") {
            let mut chunk = [0u8; 4096];
            match s.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => got.push_str(&String::from_utf8_lossy(&chunk[..n])),
                Err(_) => break,
            }
        }
        assert!(got.contains("text/event-stream"), "no sse header");
        // give the server thread a moment to enter its recv loop, then add
        std::thread::sleep(Duration::from_millis(300));
        let client = RpcClient::new(&format!("127.0.0.1:{}", server.port()));
        let r = client
            .call("downloads.add", json!({ "url": "http://127.0.0.1:1/nope" }))
            .unwrap();
        let _ = r;
        while std::time::Instant::now() < deadline && !got.contains("task-added") {
            let mut chunk = [0u8; 4096];
            match s.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => got.push_str(&String::from_utf8_lossy(&chunk[..n])),
                Err(_) => break,
            }
        }
        assert!(got.contains("text/event-stream"), "no sse header");
        assert!(got.contains("task-added"), "no task-added event: {}", got);
        server.stop();
        engine.shutdown();
    }

    /// Loopback trust boundary: shapes a web page can produce must be
    /// refused. Covers the ATKR round-1 attack primitives: no-cors
    /// `text/plain` POSTs (simple requests, no preflight), Origin-bearing
    /// requests, missing Host (HTTP/1.0 style), and DNS-rebinding hosts.
    #[test]
    fn web_page_request_shapes_are_refused() {
        let engine = start_test_engine();
        let server = RpcServer::start(engine.clone(), "127.0.0.1:0").unwrap();
        let port = server.port();

        // (request bytes, expected status substring, what it models)
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"daemon.ping","params":{}}"#;
        let rpc_len = body.len();
        let cases: Vec<(Vec<u8>, &str, &str)> = vec![
            // browser simple request: text/plain cross-site POST (no preflight)
            (
                format!(
                    "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nContent-Type: text/plain\r\nContent-Length: {l}\r\nConnection: close\r\n\r\n",
                    p = port, l = rpc_len
                )
                .into_bytes(),
                "415",
                "text/plain simple POST",
            ),
            // fetch() with an Origin header (opaque no-cors or credentialed)
            (
                format!(
                    "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nOrigin: http://evil.example\r\nContent-Type: application/json\r\nContent-Length: {l}\r\nConnection: close\r\n\r\n",
                    p = port, l = rpc_len
                )
                .into_bytes(),
                "403",
                "Origin-bearing POST",
            ),
            // sendBeacon / form post with Referer
            (
                format!(
                    "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nReferer: http://evil.example/page\r\nContent-Type: application/json\r\nContent-Length: {l}\r\nConnection: close\r\n\r\n",
                    p = port, l = rpc_len
                )
                .into_bytes(),
                "403",
                "Referer-bearing POST",
            ),
            // DNS rebinding: attacker hostname in Host with full read intent
            (
                format!(
                    "GET /events HTTP/1.1\r\nHost: evil.example:{p}\r\n\r\n",
                    p = port
                )
                .into_bytes(),
                "403",
                "rebound Host on /events",
            ),
            (
                format!(
                    "GET /health HTTP/1.1\r\nHost: evil.example:{p}\r\nConnection: close\r\n\r\n",
                    p = port
                )
                .into_bytes(),
                "403",
                "rebound Host on /health",
            ),
            // no Host at all
            (
                b"GET /health HTTP/1.0\r\nConnection: close\r\n\r\n".to_vec(),
                "403",
                "missing Host",
            ),
        ];

        for (req, want_status, what) in cases {
            let mut s = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            s.write_all(&req).unwrap();
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            assert!(
                buf.contains(&format!("{} ", want_status)),
                "{}: expected {} got: {}",
                what,
                want_status,
                buf.split('\r').next().unwrap_or("")
            );
        }

        // the shapes legitimate local clients produce still pass:
        // localhost spelling and loopback IP without port
        for host in [format!("localhost:{}", port), "127.0.0.1".to_string()] {
            let mut s = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            s.write_all(
                format!(
                    "GET /health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                    host
                )
                .as_bytes(),
            )
            .unwrap();
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            assert!(buf.contains("200 OK"), "host {:?} rejected: {}", host, buf);
        }

        server.stop();
        engine.shutdown();
    }

    #[test]
    fn host_allowed_matrix() {
        use std::net::SocketAddr;
        let lo: SocketAddr = "127.0.0.1:36990".parse().unwrap();
        assert!(host_allowed("127.0.0.1:36990", lo));
        assert!(host_allowed("127.0.0.1", lo));
        assert!(host_allowed("localhost:36990", lo));
        assert!(host_allowed("LOCALHOST:36990", lo));
        assert!(host_allowed("[::1]:36990", lo));
        assert!(!host_allowed("evil.example:36990", lo));
        assert!(!host_allowed("sub.localhost:36990", lo));
        assert!(!host_allowed("10.0.0.5:36990", lo));
        let wild: SocketAddr = "0.0.0.0:36990".parse().unwrap();
        assert!(host_allowed("192.168.1.5:36990", wild));
        assert!(host_allowed("localhost:36990", wild));
        assert!(!host_allowed("evil.example:36990", wild));
        let lan: SocketAddr = "192.168.1.5:36990".parse().unwrap();
        assert!(host_allowed("192.168.1.5:36990", lan));
        assert!(!host_allowed("evil.example:36990", lan));
        assert!(!host_allowed("localhost:36990", lan));
    }
}
