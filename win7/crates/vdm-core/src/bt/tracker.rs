//! HTTP tracker client (BEP-3 announce) plus a small in-process tracker
//! server used by the test harness and `velocitydm seed`.

use crate::bt::bencode::{self, Value};
use crate::error::{Error, Result};
use crate::http::{ClientCfg, Conn, Request};
use crate::util::{percent_decode, percent_encode_query_binary};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
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
    let interval = v.dict_get(b"interval").and_then(|i| i.as_int()).unwrap_or(1800) as u64;
    let mut peers = Vec::new();
    if let Some(p) = v.dict_get(b"peers") {
        if let Some(compact) = p.as_bytes() {
            if compact.len() % 6 != 0 {
                return Err(Error::Torrent("tracker peers length not multiple of 6".into()));
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
        complete: v.dict_get(b"complete").and_then(|c| c.as_int()).map(|c| c as u64),
        incomplete: v.dict_get(b"incomplete").and_then(|c| c.as_int()).map(|c| c as u64),
    })
}

/// Announce all trackers; returns (peers found, ok trackers, failed trackers).
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
    for t in trackers {
        if t.starts_with("udp://") || t.starts_with("dht:") {
            failures.push(format!("{} (UDP trackers not supported)", t));
            continue;
        }
        match announce(t, info_hash, peer_id, port, uploaded, downloaded, left, event, Duration::from_secs(10)) {
            Ok(resp) => {
                ok += 1;
                for p in resp.peers {
                    if !peers.contains(&p) {
                        peers.push(p);
                    }
                }
            }
            Err(e) => failures.push(format!("{} ({})", t, e)),
        }
    }
    (peers, ok, failures)
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
    let query = path.split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();
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
        let (peers, ok, failures) = announce_all(
            &[s1.url(), s2.url(), "udp://x:80/ann".to_string()],
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
}
