//! HTTP tracker client (BEP-3 announce) plus a small in-process tracker
//! server used by the test harness and `velocitydm seed`.
//!
//! Also: the UDP tracker announce path (BEP-15), dispatched from
//! [`announce_all`] for `udp://` tracker URLs.

use crate::bt::bencode::{self, Value};
use crate::error::{Error, Result};
use crate::http::{ClientCfg, Conn, Request};
use crate::util::{percent_decode, percent_encode_query_binary};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackerEvent {
    Started,
    Stopped,
    Completed,
    Periodic,
}

#[derive(Debug, Clone, Default)]
pub struct AnnounceResp {
    pub interval: u64,
    pub peers: Vec<SocketAddr>,
    pub complete: Option<u64>,
    pub incomplete: Option<u64>,
}

/// One tracker announce over HTTP.
pub fn announce(
    tracker_url: &str,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    left: u64,
    event: TrackerEvent,
    timeout: Duration,
) -> Result<AnnounceResp> {
    let sep = if tracker_url.contains('?') { '&' } else { '?' };
    let event_str = match event {
        TrackerEvent::Started => "started",
        TrackerEvent::Stopped => "stopped",
        TrackerEvent::Completed => "completed",
        TrackerEvent::Periodic => "",
    };
    let url = format!(
        "{}{}info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact=1&no_peer_id=1{}{}",
        tracker_url,
        sep,
        percent_encode_query_binary(info_hash),
        percent_encode_query_binary(peer_id),
        port,
        uploaded,
        downloaded,
        left,
        if event_str.is_empty() { "" } else { "&event=" },
        event_str
    );
    let parsed = crate::http::Url::parse(&url)?;
    let cfg = ClientCfg {
        io_timeout: Some(timeout),
        connect_timeout: Some(timeout.min(Duration::from_secs(15))),
        ..Default::default()
    };
    let mut conn = Conn::connect(&parsed, &cfg)?;
    let mut resp = conn.request(&Request::get(&parsed), &cfg)?;
    if resp.status != 200 {
        return Err(Error::Torrent(format!("tracker returned {}", resp.status)));
    }
    let body = resp.read_body(1024 * 1024)?;
    let v = bencode::decode(&body)?;
    if let Some(reason) = v.dict_get(b"failure reason").and_then(|r| r.as_str_lossy()) {
        return Err(Error::Torrent(format!("tracker failure: {}", reason)));
    }
    let interval = v
        .dict_get(b"interval")
        .and_then(|i| i.as_int())
        .unwrap_or(1800) as u64;
    let mut peers = Vec::new();
    if let Some(p) = v.dict_get(b"peers") {
        if let Some(compact) = p.as_bytes() {
            if compact.len() % 6 != 0 {
                return Err(Error::Torrent(
                    "tracker peers length not multiple of 6".into(),
                ));
            }
            for chunk in compact.chunks(6) {
                let ip = [chunk[0], chunk[1], chunk[2], chunk[3]];
                let port = u16::from_be_bytes([chunk[4], chunk[5]]);
                peers.push(SocketAddr::from((ip, port)));
            }
        } else if let Some(list) = p.as_list() {
            for entry in list {
                let ip = entry.dict_get(b"ip").and_then(|i| i.as_str_lossy());
                let port = entry.dict_get(b"port").and_then(|p| p.as_int());
                if let (Some(ip), Some(port)) = (ip, port) {
                    let addr = format!("{}:{}", ip, port);
                    if let Ok(mut addrs) = addr.to_socket_addrs() {
                        if let Some(a) = addrs.next() {
                            peers.push(a);
                        }
                    }
                }
            }
        }
    }
    Ok(AnnounceResp {
        interval,
        peers,
        complete: v
            .dict_get(b"complete")
            .and_then(|c| c.as_int())
            .map(|c| c as u64),
        incomplete: v
            .dict_get(b"incomplete")
            .and_then(|c| c.as_int())
            .map(|c| c as u64),
    })
}

// ---------------------------------------------------------------------------
// Failure-string hygiene (tracker-supplied data reaches RPC replies, task
// notices and logs)
// ---------------------------------------------------------------------------

/// Strip C0/C1 control chars + DEL. `char::is_control` covers exactly
/// U+0000-001F (C0) plus U+007F-009F (DEL + C1).
fn strip_control_chars(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Render a tracker URL for a failure string: byte-truncate at 256 bytes
/// BEFORE `from_utf8_lossy` (never slice a `String` mid-char), then strip
/// C0/C1 + DEL.
fn render_tracker_url(url: &str) -> String {
    const URL_RENDER_CAP: usize = 256;
    let bytes = url.as_bytes();
    let capped: &[u8] = if bytes.len() > URL_RENDER_CAP {
        &bytes[..URL_RENDER_CAP]
    } else {
        bytes
    };
    strip_control_chars(&String::from_utf8_lossy(capped))
}

/// Per-announce-round tracker cap: at most the first 32 tracker URLs are
/// processed per round; excess entries are skipped with a failure note.
/// (An 8 MiB magnet body can carry thousands of `tr=` entries; 32 x 20 s
/// worst case <= the unbounded HTTP worst case for the same magnet.)
const MAX_TRACKERS_PER_ROUND: usize = 32;

/// Announce all trackers; returns (peers found, ok trackers, failed trackers).
#[allow(clippy::too_many_arguments)] // fixed BEP-3 announce field set
pub fn announce_all(
    trackers: &[String],
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    left: u64,
    event: TrackerEvent,
) -> (Vec<SocketAddr>, usize, Vec<String>) {
    let mut peers = Vec::new();
    let mut ok = 0;
    let mut failures = Vec::new();
    for (i, t) in trackers.iter().enumerate() {
        if i >= MAX_TRACKERS_PER_ROUND {
            failures.push(format!(
                "{} (excess trackers skipped: round cap)",
                render_tracker_url(t)
            ));
            continue;
        }
        if t.starts_with("dht:") {
            // Not a tracker — DHT is a separate peer source handled by the
            // engine; there is no per-URL UDP claim to make here.
            failures.push(format!(
                "{} (not a tracker; DHT handled separately)",
                render_tracker_url(t)
            ));
            continue;
        }
        if t.starts_with("udp://") {
            match udp_announce(t, info_hash, peer_id, port, uploaded, downloaded, left, event) {
                Ok(resp) => {
                    ok += 1;
                    for p in resp.peers {
                        if !peers.contains(&p) {
                            peers.push(p);
                        }
                    }
                }
                Err(e) => failures.push(format!("{} ({})", render_tracker_url(t), e)),
            }
            continue;
        }
        match announce(
            t,
            info_hash,
            peer_id,
            port,
            uploaded,
            downloaded,
            left,
            event,
            Duration::from_secs(10),
        ) {
            Ok(resp) => {
                ok += 1;
                for p in resp.peers {
                    if !peers.contains(&p) {
                        peers.push(p);
                    }
                }
            }
            Err(e) => failures.push(format!("{} ({})", render_tracker_url(t), e)),
        }
    }
    (peers, ok, failures)
}

// ---------------------------------------------------------------------------
// UDP tracker announce (BEP-15)
// ---------------------------------------------------------------------------

/// BEP-15 protocol constant.
const UDP_MAGIC: u64 = 0x0000_0417_2710_1980;
/// Response recv buffer bound (DHT loop precedent): <= 341 peers per reply.
const UDP_RECV_BUF: usize = 2048;
/// Pinned timeouts (gate condition): connect wait 1 = 5 s, exactly one
/// retransmit with connect wait 2 = 5 s, announce wait = 10 s — per-tracker
/// wall clock <= 20 s.
const UDP_CONNECT_WAIT: Duration = Duration::from_secs(5);
const UDP_ANNOUNCE_WAIT: Duration = Duration::from_secs(10);

/// One tracker announce over UDP (BEP-15).
#[allow(clippy::too_many_arguments)] // fixed BEP-15 announce field set
pub fn udp_announce(
    tracker_url: &str,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    left: u64,
    event: TrackerEvent,
) -> Result<AnnounceResp> {
    udp_announce_timed(
        tracker_url,
        info_hash,
        peer_id,
        port,
        uploaded,
        downloaded,
        left,
        event,
        UDP_CONNECT_WAIT,
        UDP_ANNOUNCE_WAIT,
    )
}

/// Timeout-parameterized UDP announce. The pinned bound lives in
/// [`udp_announce`]; tests drive short waits against loopback harnesses.
#[allow(clippy::too_many_arguments)] // fixed BEP-15 announce field set
fn udp_announce_timed(
    tracker_url: &str,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    left: u64,
    event: TrackerEvent,
    connect_wait: Duration,
    announce_wait: Duration,
) -> Result<AnnounceResp> {
    // Parse/resolve first: URL-invalid, non-unicast and v6-only targets are
    // refused before any datagram is sent.
    let target = udp_resolve(tracker_url)?;
    let sock = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| Error::Torrent(format!("udp tracker: bind failed: {}", e)))?;
    // Connected-socket source pinning (primary authenticity control):
    // connect AND announce responses are received only on this connected
    // 5-tuple; the OS drops datagrams from any other source. Transaction-id
    // randomness is defense-in-depth, not the primary control.
    sock.connect(target)
        .map_err(|e| Error::Torrent(format!("udp tracker: connect failed: {}", e)))?;
    let conn_id = udp_connect(&sock, connect_wait)?;
    // Announce request: action 1, 98-byte fixed body.
    let tid = udp_new_tid();
    let mut req = Vec::with_capacity(98);
    req.extend_from_slice(&conn_id.to_be_bytes());
    req.extend_from_slice(&1u32.to_be_bytes());
    req.extend_from_slice(&tid);
    req.extend_from_slice(info_hash);
    req.extend_from_slice(peer_id);
    req.extend_from_slice(&downloaded.to_be_bytes());
    req.extend_from_slice(&left.to_be_bytes());
    req.extend_from_slice(&uploaded.to_be_bytes());
    req.extend_from_slice(&udp_event_code(event).to_be_bytes());
    req.extend_from_slice(&0u32.to_be_bytes()); // source ip
    req.extend_from_slice(&0u32.to_be_bytes()); // key
    req.extend_from_slice(&(-1i32).to_be_bytes()); // num_want
    req.extend_from_slice(&port.to_be_bytes()); // the existing announce_port
    sock.set_read_timeout(Some(announce_wait))
        .map_err(|e| Error::Torrent(format!("udp tracker: set_read_timeout: {}", e)))?;
    sock.send(&req)
        .map_err(|e| Error::Torrent(format!("udp tracker: send failed: {}", e)))?;
    let mut buf = [0u8; UDP_RECV_BUF];
    let n = udp_recv_once(&sock, &mut buf).map_err(|e| {
        Error::Torrent(format!("udp tracker: announce timeout (no reply): {}", e))
    })?;
    parse_announce_reply(&buf[..n], &tid)
}

/// Connect handshake (action 0): send the 16-byte request, wait
/// `connect_wait`; on timeout exactly ONE retransmit with the same datagram
/// and the same transaction id, then wait `connect_wait` again. The first
/// valid response wins; late/duplicate replies are idempotent.
fn udp_connect(sock: &UdpSocket, connect_wait: Duration) -> Result<u64> {
    let tid = udp_new_tid();
    let mut req = [0u8; 16];
    req[0..8].copy_from_slice(&UDP_MAGIC.to_be_bytes());
    req[8..12].copy_from_slice(&0u32.to_be_bytes()); // action connect
    req[12..16].copy_from_slice(&tid);
    sock.set_read_timeout(Some(connect_wait))
        .map_err(|e| Error::Torrent(format!("udp tracker: set_read_timeout: {}", e)))?;
    let mut buf = [0u8; UDP_RECV_BUF];
    for _attempt in 0..2u8 {
        sock.send(&req)
            .map_err(|e| Error::Torrent(format!("udp tracker: send failed: {}", e)))?;
        match udp_recv_once(sock, &mut buf) {
            Ok(n) => return parse_connect_reply(&buf[..n], &tid),
            // Wait 1 (attempt 0): the attempt-1 send below is the single
            // retransmit. Wait 2 (attempt 1): the bound is missed — fail.
            Err(e) if e.kind() == ErrorKind::TimedOut => {}
            Err(e) => {
                return Err(Error::Torrent(format!(
                    "udp tracker: connect recv failed: {}",
                    e
                )))
            }
        }
    }
    Err(Error::Torrent(
        "udp tracker: connect timeout (no reply after retransmit)".into(),
    ))
}

/// One recv on the connected socket. Windows surfaces WSAECONNRESET on the
/// next recv after a datagram sent to an unbound port — classified as
/// "no reply" (retransmit/failure path), never a panic.
fn udp_recv_once(sock: &UdpSocket, buf: &mut [u8]) -> std::io::Result<usize> {
    match sock.recv(buf) {
        Ok(n) => Ok(n),
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset
            ) =>
        {
            Err(std::io::Error::new(ErrorKind::TimedOut, "no reply"))
        }
        Err(e) => Err(e),
    }
}

/// 4 transaction-id bytes from `util::random_bytes` per request —
/// explicitly NOT the DHT module's sequential counter (`alloc_tid`).
fn udp_new_tid() -> [u8; 4] {
    let buf = crate::util::random_bytes(4);
    [buf[0], buf[1], buf[2], buf[3]]
}

/// BEP-15 event code, mapped from `TrackerEvent` exactly as the HTTP path
/// maps it to strings: 0 none / 1 completed / 2 started / 3 stopped.
fn udp_event_code(event: TrackerEvent) -> i32 {
    match event {
        TrackerEvent::Completed => 1,
        TrackerEvent::Started => 2,
        TrackerEvent::Stopped => 3,
        TrackerEvent::Periodic => 0,
    }
}

/// Parse `udp://host:port[/path]` and select the announce target.
///
/// - The path component is irrelevant to BEP-15 (routing is by port);
///   `/announce` and `/scrape` suffixes are accepted and ignored.
/// - Address selection (pinned): resolve the host, filter to IPv4, use the
///   first V4 candidate in resolution order. A resolved set with no IPv4
///   address (v6-only resolution, or a literal `udp://[::1]:...` URL) returns
///   the classified ipv6-not-supported error BEFORE any datagram is sent.
/// - Non-unicast destinations (multicast 224.0.0.0/4 + ff00::/8,
///   255.255.255.255, 0.0.0.0, ::) are refused at parse time. Unicast
///   loopback/private targets stay permitted (HTTP-path parity).
fn udp_resolve(url: &str) -> Result<SocketAddr> {
    let invalid =
        |reason: &str| -> Error { Error::Torrent(format!("udp tracker url invalid: {}", reason)) };
    let rest = url
        .strip_prefix("udp://")
        .ok_or_else(|| invalid("missing udp:// scheme"))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return Err(invalid("missing host"));
    }
    let (host, port_str) = match authority.rfind(':') {
        Some(i) => (&authority[..i], &authority[i + 1..]),
        None => return Err(invalid("missing port")),
    };
    if host.is_empty() {
        return Err(invalid("missing host"));
    }
    let port: u16 = if port_str.is_empty() {
        return Err(invalid("missing port"));
    } else {
        port_str.parse().map_err(|_| invalid("invalid port"))?
    };
    if port == 0 {
        return Err(invalid("invalid port"));
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        // Bracketed IPv6 literal (e.g. udp://[::1]:80/ann).
        let v6: Ipv6Addr = inner.parse().map_err(|_| invalid("invalid host"))?;
        return v6_target(v6);
    }
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        // Literal IPv4: refused/selected without any DNS I/O.
        if !is_unicast_v4(&v4) {
            return Err(invalid("non-unicast target"));
        }
        return Ok(SocketAddr::from((v4, port)));
    }
    if host.contains(':') {
        // Unbracketed IPv6 literal.
        let v6: Ipv6Addr = host.parse().map_err(|_| invalid("invalid host"))?;
        return v6_target(v6);
    }
    // Hostname: resolve, iterate candidates in resolution order, first V4 wins.
    let mut saw_candidate = false;
    for a in (host, port)
        .to_socket_addrs()
        .map_err(|_| invalid("unresolvable host"))?
    {
        saw_candidate = true;
        if let IpAddr::V4(v4) = a.ip() {
            if !is_unicast_v4(&v4) {
                return Err(invalid("non-unicast target"));
            }
            return Ok(SocketAddr::new(IpAddr::V4(v4), a.port()));
        }
    }
    if saw_candidate {
        // Resolved set contains no IPv4 address (v6-only resolution).
        Err(Error::Torrent(
            "udp tracker: ipv6 announce not supported".into(),
        ))
    } else {
        Err(invalid("unresolvable host"))
    }
}

/// IPv6 targets: multicast (ff00::/8) and `::` are non-unicast refusals;
/// every other v6 target is refused with the pinned ipv6-not-supported
/// error (a message, not a protocol claim — BEP-07 governs IPv6 *HTTP*
/// announces; we do not do UDP-over-v6 yet).
fn v6_target(v6: Ipv6Addr) -> Result<SocketAddr> {
    if v6.is_multicast() || v6.is_unspecified() {
        Err(Error::Torrent(
            "udp tracker url invalid: non-unicast target".into(),
        ))
    } else {
        Err(Error::Torrent(
            "udp tracker: ipv6 announce not supported".into(),
        ))
    }
}

/// Multicast (224.0.0.0/4), broadcast (255.255.255.255) and unspecified
/// (0.0.0.0) are refused. Unicast loopback/private stay permitted.
fn is_unicast_v4(v4: &Ipv4Addr) -> bool {
    !(v4.is_multicast() || v4.is_broadcast() || v4.is_unspecified())
}

/// Length-checked big-endian field extraction (the `get()` pattern the
/// HTTP compact path uses): `None` = datagram too short for the field.
fn be_u32(buf: &[u8], off: usize) -> Option<u32> {
    let s = buf.get(off..off + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn be_u64(buf: &[u8], off: usize) -> Option<u64> {
    let s = buf.get(off..off + 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(u64::from_be_bytes(a))
}

/// BEP-15 error reply: action 3, transaction echo, then the text message —
/// there is NO error-code field, the message starts at byte 8. Surfaced as
/// `udp tracker error: <message>` (sanitized: C0/C1 + DEL stripped).
fn error_reply(buf: &[u8], tid: &[u8; 4]) -> Error {
    let echoed = be_u32(buf, 4)
        .map(|e| e == u32::from_be_bytes(*tid))
        .unwrap_or(false);
    if !echoed {
        return Error::Torrent("udp tracker: transaction id mismatch on error reply".into());
    }
    let raw = buf.get(8..).unwrap_or(&[]);
    let msg = strip_control_chars(&String::from_utf8_lossy(raw));
    Error::Torrent(format!("udp tracker error: {}", msg))
}

/// Connect response: action echo (0), transaction echo, 8-byte connection
/// id (minimum length 16 bytes). Mismatched action or transaction id is a
/// classified error. Error replies (action 3, minimum 8 bytes) surface the
/// tracker message.
fn parse_connect_reply(buf: &[u8], tid: &[u8; 4]) -> Result<u64> {
    let action = be_u32(buf, 0)
        .ok_or_else(|| Error::Torrent("udp tracker: short connect response".into()))?;
    if action == 3 {
        return Err(error_reply(buf, tid));
    }
    if action != 0 {
        return Err(Error::Torrent(format!(
            "udp tracker: unexpected connect action {}",
            action
        )));
    }
    match be_u32(buf, 4) {
        Some(echo) if echo == u32::from_be_bytes(*tid) => {}
        Some(_) => {
            return Err(Error::Torrent(
                "udp tracker: connect transaction id mismatch".into(),
            ))
        }
        None => return Err(Error::Torrent("udp tracker: short connect response".into())),
    }
    be_u64(buf, 8).ok_or_else(|| Error::Torrent("udp tracker: short connect response".into()))
}

/// Announce response (minimum length 20 bytes): action echo (1),
/// transaction echo, interval i32, leechers i32, seeders i32, then the
/// compact 6-byte peer list (4 ip + 2 port each). Field mapping: seeders ->
/// `complete`, leechers -> `incomplete` (negative => None), interval i32 ->
/// u64 saturating (missing => 1800, the HTTP default — unreachable after
/// the 20-byte minimum check, kept for the mapping contract). interval/
/// leechers/seeders are parsed but UNUSED for scheduling (parity with the
/// HTTP path). Peer-list parsing mirrors the HTTP compact path: the
/// length-multiple-of-6 check happens BEFORE any indexing.
fn parse_announce_reply(buf: &[u8], tid: &[u8; 4]) -> Result<AnnounceResp> {
    let action = be_u32(buf, 0)
        .ok_or_else(|| Error::Torrent("udp tracker: short announce response".into()))?;
    if action == 3 {
        return Err(error_reply(buf, tid));
    }
    if action != 1 {
        return Err(Error::Torrent(format!(
            "udp tracker: unexpected announce action {}",
            action
        )));
    }
    let echo = be_u32(buf, 4)
        .ok_or_else(|| Error::Torrent("udp tracker: short announce response".into()))?;
    if echo != u32::from_be_bytes(*tid) {
        return Err(Error::Torrent(
            "udp tracker: announce transaction id mismatch".into(),
        ));
    }
    if buf.len() < 20 {
        return Err(Error::Torrent(
            "udp tracker: short announce response".into(),
        ));
    }
    let interval = be_u32(buf, 8)
        .map(|raw| (raw as i32).max(0) as u64)
        .unwrap_or(1800);
    let leechers = be_u32(buf, 12).map(|raw| raw as i32);
    let seeders = be_u32(buf, 16).map(|raw| raw as i32);
    let tail = &buf[20..];
    if tail.len() % 6 != 0 {
        return Err(Error::Torrent(
            "udp tracker: peers length not multiple of 6".into(),
        ));
    }
    let mut peers = Vec::with_capacity(tail.len() / 6);
    for chunk in tail.chunks(6) {
        let ip = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let p = u16::from_be_bytes([chunk[4], chunk[5]]);
        peers.push(SocketAddr::from((ip, p)));
    }
    Ok(AnnounceResp {
        interval,
        peers,
        complete: seeders.filter(|s| *s >= 0).map(|s| s as u64),
        incomplete: leechers.filter(|l| *l >= 0).map(|l| l as u64),
    })
}

// ---------------------------------------------------------------------------
// In-process tracker server (test harness + `velocitydm seed`)
// ---------------------------------------------------------------------------

type Registry = Arc<Mutex<HashMap<[u8; 20], Vec<SocketAddr>>>>;

pub struct TrackerServer {
    pub addr: SocketAddr,
    registry: Registry,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TrackerServer {
    /// Start on `bind` (use port 0 for ephemeral).
    pub fn start(bind: &str) -> Result<TrackerServer> {
        let listener = TcpListener::bind(bind)
            .map_err(|e| Error::Torrent(format!("tracker bind {}: {}", bind, e)))?;
        let addr = listener
            .local_addr()
            .map_err(|e| Error::Torrent(format!("tracker local_addr: {}", e)))?;
        let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reg = registry.clone();
        let stop_flag = stop.clone();
        let handle = std::thread::spawn(move || {
            listener.set_nonblocking(true).ok();
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let reg = reg.clone();
                        std::thread::spawn(move || handle_tracker_conn(stream, reg));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(TrackerServer {
            addr,
            registry,
            stop,
            handle: Some(handle),
        })
    }

    pub fn url(&self) -> String {
        format!("http://{}/announce", self.addr)
    }

    /// Register a seed for `info_hash`.
    pub fn register(&self, info_hash: [u8; 20], addr: SocketAddr) {
        let mut reg = self.registry.lock().unwrap();
        let list = reg.entry(info_hash).or_default();
        if !list.contains(&addr) {
            list.push(addr);
        }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn handle_tracker_conn(mut stream: TcpStream, registry: Registry) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    // read request head
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return,
            Ok(_) => {
                buf.push(byte[0]);
                if buf.ends_with(b"\r\n\r\n") || buf.ends_with(b"\n\n") {
                    break;
                }
                if buf.len() > 16384 {
                    return;
                }
            }
            Err(_) => return,
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let first_line = text.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let _method = parts.next();
    let path = parts.next().unwrap_or("/").to_string();
    // parse query
    let query = path
        .split_once('?')
        .map(|(_, q)| q.to_string())
        .unwrap_or_default();
    let mut info_hash: Option<[u8; 20]> = None;
    let mut event = String::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        match k {
            "info_hash" => {
                // binary value: decode to raw bytes (never through lossy UTF-8)
                let decoded = crate::util::percent_decode_bytes(v);
                if decoded.len() == 20 {
                    let mut h = [0u8; 20];
                    h.copy_from_slice(&decoded);
                    info_hash = Some(h);
                }
            }
            "event" => event = percent_decode(v),
            _ => {}
        }
    }
    let (peers, complete, incomplete) = {
        let reg = registry.lock().unwrap();
        match info_hash {
            Some(h) => {
                let list = reg.get(&h).cloned().unwrap_or_default();
                let complete = list.len() as i64;
                (list, complete, 0i64)
            }
            None => (Vec::new(), 0, 0),
        }
    };
    let _ = event; // stopped/completed not tracked by the harness registry
    let mut compact = Vec::with_capacity(peers.len() * 6);
    for p in &peers {
        if let std::net::IpAddr::V4(ip) = p.ip() {
            compact.extend_from_slice(&ip.octets());
        } else {
            continue;
        }
        compact.extend_from_slice(&p.port().to_be_bytes());
    }
    let body = bencode::encode(&Value::Dict(vec![
        (b"complete".to_vec(), Value::Int(complete)),
        (b"incomplete".to_vec(), Value::Int(incomplete)),
        (b"interval".to_vec(), Value::Int(5)),
        (b"peers".to_vec(), Value::Str(compact)),
    ]));
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.write_all(&body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_server_roundtrip() {
        let server = TrackerServer::start("127.0.0.1:0").unwrap();
        let hash = [7u8; 20];
        let seed_addr: SocketAddr = "127.0.0.1:55555".parse().unwrap();
        server.register(hash, seed_addr);
        let peer_id = crate::util::new_peer_id();
        let resp = announce(
            &server.url(),
            &hash,
            &peer_id,
            6881,
            0,
            0,
            1000,
            TrackerEvent::Started,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(resp.peers, vec![seed_addr]);
        assert_eq!(resp.interval, 5);
        assert_eq!(resp.complete, Some(1));
        // unknown hash -> no peers
        let resp2 = announce(
            &server.url(),
            &[9u8; 20],
            &peer_id,
            6881,
            0,
            0,
            0,
            TrackerEvent::Started,
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(resp2.peers.is_empty());
        server.stop();
    }

    #[test]
    fn announce_all_aggregates() {
        let s1 = TrackerServer::start("127.0.0.1:0").unwrap();
        let s2 = TrackerServer::start("127.0.0.1:0").unwrap();
        let hash = [3u8; 20];
        s1.register(hash, "127.0.0.1:1111".parse().unwrap());
        s2.register(hash, "127.0.0.1:2222".parse().unwrap());
        // Portless udp:// dummy: parse-stage failure, zero DNS/network I/O.
        let (peers, ok, failures) = announce_all(
            &[s1.url(), s2.url(), "udp://x/ann".to_string()],
            &hash,
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            5,
            TrackerEvent::Started,
        );
        assert_eq!(peers.len(), 2);
        assert_eq!(ok, 2);
        assert_eq!(failures.len(), 1);
        s1.stop();
        s2.stop();
    }

    /// Runner firewalls occasionally reset loopback connections mid-test.
    /// Retry the whole body on connection-level panics; real assertion
    /// failures re-raise.
    fn flaky_loopback(mut f: impl FnMut() + std::panic::RefUnwindSafe) {
        let mut last = String::new();
        for attempt in 0..5 {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut f)) {
                Ok(()) => return,
                Err(p) => {
                    last = p
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default();
                    let conn_fail = last.contains("reset")
                        || last.contains("aborted")
                        || last.contains("closed connection")
                        || last.contains("Connection");
                    if !conn_fail {
                        std::panic::resume_unwind(p);
                    }
                    eprintln!("loopback attempt {} failed ({}); retrying", attempt, last);
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }
        panic!("loopback test failed after 5 attempts: {}", last);
    }

    // -------------------------------------------------------------------
    // UDP (BEP-15) test harness — in-process tracker speaking minimal
    // BEP-15 on loopback (mirrors the TrackerServer harness pattern).
    // Binds/connects literal 127.0.0.1, never `localhost`.
    // -------------------------------------------------------------------

    #[derive(Clone, Copy, PartialEq)]
    enum UdpMode {
        /// Full connect + announce conversation.
        Normal,
        /// Drop the first received datagram (retransmit witness).
        DropFirst,
        /// Reply to connect with a too-short datagram (malformed witness).
        ShortReply,
        /// Reply to connect with an action-3 error (error witness).
        Action3,
        /// Reply to connect with a mismatched transaction id (mismatch witness).
        WrongTid,
        /// Announce reply carries a peer tail that is not a multiple of 6.
        TruncatedPeers,
        /// Never reply (timeout witness).
        Silent,
    }

    struct UdpReplyCfg {
        interval: i32,
        leechers: i32,
        seeders: i32,
        peers: Vec<SocketAddr>,
    }

    impl Default for UdpReplyCfg {
        fn default() -> Self {
            UdpReplyCfg {
                interval: 5,
                leechers: 1,
                seeders: 2,
                peers: Vec::new(),
            }
        }
    }

    struct UdpTrackerTestServer {
        addr: SocketAddr,
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl UdpTrackerTestServer {
        fn start(mode: UdpMode, reply: UdpReplyCfg) -> UdpTrackerTestServer {
            let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
            let addr = sock.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_flag = stop.clone();
            let handle = std::thread::spawn(move || {
                sock.set_read_timeout(Some(Duration::from_millis(50))).ok();
                let mut dropped_first = false;
                let mut buf = [0u8; 2048];
                loop {
                    if stop_flag.load(Ordering::Relaxed) {
                        break;
                    }
                    let (n, src) = match sock.recv_from(&mut buf) {
                        Ok(x) => x,
                        Err(ref e)
                            if e.kind() == ErrorKind::WouldBlock
                                || e.kind() == ErrorKind::TimedOut =>
                        {
                            continue
                        }
                        Err(_) => break,
                    };
                    let msg = buf[..n].to_vec();
                    if let Some(out) = udp_test_serve(mode, &reply, &msg, &mut dropped_first) {
                        let _ = sock.send_to(&out, src);
                    }
                }
            });
            UdpTrackerTestServer {
                addr,
                stop,
                handle: Some(handle),
            }
        }

        fn url(&self) -> String {
            format!("udp://{}/announce", self.addr)
        }

        fn stop(mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// Minimal BEP-15 responder: one datagram in, optional reply out.
    fn udp_test_serve(
        mode: UdpMode,
        cfg: &UdpReplyCfg,
        msg: &[u8],
        dropped_first: &mut bool,
    ) -> Option<Vec<u8>> {
        if mode == UdpMode::Silent {
            return None;
        }
        if msg.len() < 16 {
            return None;
        }
        let action = be_u32(msg, 8)?;
        let tid = [msg[12], msg[13], msg[14], msg[15]];
        if action == 0 {
            // connect request
            if mode == UdpMode::DropFirst && !*dropped_first {
                *dropped_first = true;
                return None;
            }
            if mode == UdpMode::ShortReply {
                // 8 bytes: action echo + tid, no connection id (< 16).
                let mut out = Vec::with_capacity(8);
                out.extend_from_slice(&0u32.to_be_bytes());
                out.extend_from_slice(&tid);
                return Some(out);
            }
            if mode == UdpMode::Action3 {
                // action 3 + tid echo + message carrying C0/DEL controls
                let mut out = Vec::with_capacity(8 + 19);
                out.extend_from_slice(&3u32.to_be_bytes());
                out.extend_from_slice(&tid);
                out.extend_from_slice(b"boom\x07\x7f tracker said no");
                return Some(out);
            }
            if mode == UdpMode::WrongTid {
                let wrong = u32::from_be_bytes(tid).wrapping_add(1);
                let mut out = Vec::with_capacity(16);
                out.extend_from_slice(&0u32.to_be_bytes());
                out.extend_from_slice(&wrong.to_be_bytes());
                out.extend_from_slice(&0x5355_5350_414e_4e55u64.to_be_bytes());
                return Some(out);
            }
            // Normal / DropFirst(after the drop) / TruncatedPeers
            let mut out = Vec::with_capacity(16);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&tid);
            out.extend_from_slice(&0x434f_4e4e_4944_3131u64.to_be_bytes());
            return Some(out);
        }
        if action == 1 {
            // announce request
            let mut out = Vec::new();
            out.extend_from_slice(&1u32.to_be_bytes());
            out.extend_from_slice(&tid);
            out.extend_from_slice(&cfg.interval.to_be_bytes());
            out.extend_from_slice(&cfg.leechers.to_be_bytes());
            out.extend_from_slice(&cfg.seeders.to_be_bytes());
            for p in &cfg.peers {
                if let IpAddr::V4(ip) = p.ip() {
                    out.extend_from_slice(&ip.octets());
                    out.extend_from_slice(&p.port().to_be_bytes());
                }
            }
            if mode == UdpMode::TruncatedPeers {
                out.extend_from_slice(&[9u8; 7]); // 7-byte tail: not a multiple of 6
            }
            return Some(out);
        }
        None
    }

    #[test]
    fn udp_tracker_roundtrip() {
        let server = UdpTrackerTestServer::start(
            UdpMode::Normal,
            UdpReplyCfg {
                interval: 300,
                leechers: 7,
                seeders: 3,
                peers: vec!["203.0.113.10:33445".parse().unwrap()],
            },
        );
        let hash = [11u8; 20];
        let peer_id = crate::util::new_peer_id();
        let resp = udp_announce(
            &server.url(),
            &hash,
            &peer_id,
            6881,
            100,
            200,
            300,
            TrackerEvent::Started,
        )
        .unwrap();
        assert_eq!(resp.peers, vec!["203.0.113.10:33445".parse().unwrap()]);
        assert_eq!(resp.interval, 300);
        assert_eq!(resp.complete, Some(3)); // seeders -> complete
        assert_eq!(resp.incomplete, Some(7)); // leechers -> incomplete
        server.stop();
    }

    #[test]
    fn udp_tracker_negative_fields_map() {
        let server = UdpTrackerTestServer::start(
            UdpMode::Normal,
            UdpReplyCfg {
                interval: -5,
                leechers: -1,
                seeders: -3,
                peers: Vec::new(),
            },
        );
        let resp = udp_announce(
            &server.url(),
            &[18u8; 20],
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            0,
            TrackerEvent::Started,
        )
        .unwrap();
        assert_eq!(resp.interval, 0); // i32 -> u64 saturating
        assert_eq!(resp.complete, None); // negative seeders -> None
        assert_eq!(resp.incomplete, None); // negative leechers -> None
        server.stop();
    }

    #[test]
    fn udp_tracker_retransmit_after_drop() {
        // Drop-first witness: the first datagram is swallowed; the client
        // retransmits (same datagram, same transaction id) and completes
        // within the pinned 20 s bound (5 s wait 1 + 5 s wait 2 + announce).
        let server = UdpTrackerTestServer::start(
            UdpMode::DropFirst,
            UdpReplyCfg {
                peers: vec!["203.0.113.20:33446".parse().unwrap()],
                ..Default::default()
            },
        );
        let resp = udp_announce(
            &server.url(),
            &[12u8; 20],
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            10,
            TrackerEvent::Started,
        )
        .unwrap();
        assert_eq!(resp.peers.len(), 1);
        server.stop();
    }

    #[test]
    fn udp_tracker_short_response() {
        let server = UdpTrackerTestServer::start(UdpMode::ShortReply, UdpReplyCfg::default());
        let err = udp_announce(
            &server.url(),
            &[13u8; 20],
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            0,
            TrackerEvent::Started,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Torrent(_)), "got: {:?}", err);
        assert!(err.to_string().contains("short"), "got: {}", err);
        server.stop();
    }

    #[test]
    fn udp_tracker_error_reply_sanitized() {
        let server = UdpTrackerTestServer::start(UdpMode::Action3, UdpReplyCfg::default());
        let err = udp_announce(
            &server.url(),
            &[14u8; 20],
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            0,
            TrackerEvent::Started,
        )
        .unwrap_err();
        let s = err.to_string();
        assert!(s.contains("udp tracker error:"), "got: {}", s);
        assert!(s.contains("tracker said no"), "got: {}", s);
        assert!(
            !s.chars().any(|c| c.is_control()),
            "control chars survived: {:?}",
            s
        );
        server.stop();
    }

    #[test]
    fn udp_tracker_wrong_transaction_id() {
        let server = UdpTrackerTestServer::start(UdpMode::WrongTid, UdpReplyCfg::default());
        let err = udp_announce(
            &server.url(),
            &[19u8; 20],
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            0,
            TrackerEvent::Started,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Torrent(_)), "got: {:?}", err);
        assert!(
            err.to_string().contains("transaction id mismatch"),
            "got: {}",
            err
        );
        server.stop();
    }

    #[test]
    fn udp_tracker_truncated_peer_tail() {
        let server = UdpTrackerTestServer::start(
            UdpMode::TruncatedPeers,
            UdpReplyCfg {
                peers: vec!["203.0.113.30:33447".parse().unwrap()],
                ..Default::default()
            },
        );
        let err = udp_announce(
            &server.url(),
            &[20u8; 20],
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            0,
            TrackerEvent::Started,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Torrent(_)), "got: {:?}", err);
        assert!(err.to_string().contains("not multiple of 6"), "got: {}", err);
        server.stop();
    }

    #[test]
    fn udp_tracker_timeout_no_reply() {
        // Silent tracker: connect wait 1, exactly one retransmit, wait 2,
        // then a classified failure. Short waits keep the test fast; the
        // pinned 5/5/10 bound is exercised by retransmit_after_drop.
        let server = UdpTrackerTestServer::start(UdpMode::Silent, UdpReplyCfg::default());
        let err = udp_announce_timed(
            &server.url(),
            &[15u8; 20],
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            0,
            TrackerEvent::Started,
            Duration::from_millis(100),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Torrent(_)), "got: {:?}", err);
        assert!(err.to_string().contains("no reply"), "got: {}", err);
        server.stop();
    }

    #[test]
    fn udp_tracker_invalid_urls_zero_io() {
        let peer_id = crate::util::new_peer_id();
        let hash = [21u8; 20];
        let cases = [
            ("udp://x/ann", "missing port"),                  // portless URL
            ("udp://127.0.0.1:notaport/ann", "invalid port"), // non-numeric port
            ("udp://224.0.0.1:80/ann", "non-unicast target"), // multicast
            ("udp://255.255.255.255:80/ann", "non-unicast target"), // broadcast
            ("udp://0.0.0.0:80/ann", "non-unicast target"),   // unspecified
            ("udp://[::1]:80/ann", "ipv6 announce not supported"), // v6-only literal
        ];
        let t0 = std::time::Instant::now();
        for (url, needle) in cases.iter() {
            let err = udp_announce(url, &hash, &peer_id, 6881, 0, 0, 0, TrackerEvent::Started)
                .unwrap_err();
            assert!(matches!(err, Error::Torrent(_)), "{}: {:?}", url, err);
            assert!(err.to_string().contains(needle), "{}: {}", url, err);
        }
        // Parse-stage refusals: zero network I/O (well under one timeout wait).
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "URL refusals must not touch the network"
        );
    }

    #[test]
    fn announce_all_mixed_udp_and_http() {
        flaky_loopback(|| {
            let http = TrackerServer::start("127.0.0.1:0").unwrap();
            let udp = UdpTrackerTestServer::start(
                UdpMode::Normal,
                UdpReplyCfg {
                    peers: vec!["203.0.113.40:33448".parse().unwrap()],
                    ..Default::default()
                },
            );
            let hash = [16u8; 20];
            http.register(hash, "127.0.0.1:1111".parse().unwrap());
            let (peers, ok, failures) = announce_all(
                &[http.url(), udp.url()],
                &hash,
                &crate::util::new_peer_id(),
                6881,
                0,
                0,
                5,
                TrackerEvent::Started,
            );
            assert_eq!(ok, 2, "both trackers counted OK; failures: {:?}", failures);
            assert!(failures.is_empty(), "failures: {:?}", failures);
            assert!(peers.contains(&"127.0.0.1:1111".parse().unwrap()));
            assert!(peers.contains(&"203.0.113.40:33448".parse().unwrap()));
            assert_eq!(peers.len(), 2);
            http.stop();
            udp.stop();
        });
    }

    #[test]
    fn announce_all_round_cap_skips_excess() {
        // 34 entries: the first 32 are processed (each fails fast at the
        // parse stage — portless URL, zero network I/O), the remaining 2
        // are skipped with the round-cap note.
        let trackers: Vec<String> = (0..34).map(|_| "udp://x/ann".to_string()).collect();
        let (peers, ok, failures) = announce_all(
            &trackers,
            &[17u8; 20],
            &crate::util::new_peer_id(),
            6881,
            0,
            0,
            0,
            TrackerEvent::Started,
        );
        assert_eq!(ok, 0);
        assert!(peers.is_empty());
        assert_eq!(failures.len(), 34);
        for f in &failures[..32] {
            assert!(f.contains("udp tracker url invalid"), "got: {}", f);
            assert!(!f.contains("excess trackers skipped"), "got: {}", f);
        }
        for f in &failures[32..] {
            assert!(f.contains("excess trackers skipped: round cap"), "got: {}", f);
        }
    }
}
