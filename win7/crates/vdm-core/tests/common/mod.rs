//! Shared integration-test HTTP server with Range, flaky, slow,
//! inconsistent-edge and connection-degradation paths. Each test binary
//! uses a subset of the knobs, so unused-field warnings are expected and
//! allowed.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Served-range coverage map (S11-i B1): path -> served byte intervals.
type Coverage = HashMap<String, Vec<(u64, u64)>>;

pub struct TestServer {
    pub addr: SocketAddr,
    pub files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// paths starting with "/flaky" fail the first N requests globally
    pub flaky_remaining: Arc<AtomicU32>,
    /// ms to sleep per 4 KiB written for paths under "/slow"
    pub slow_ms: Arc<AtomicU32>,
    /// S11-i B1 e2e: when set, the first range request whose bytes were
    /// already fully served before (by prior range responses) is served
    /// with every byte flipped — the deterministic "inconsistent edge".
    pub flip_first_covered_repetition: Arc<AtomicBool>,
    /// S11-i B2 e2e: when > 0, responses beyond the first N range requests
    /// FOR THE PATH (globally, any connection) get `slow_ms` applied — the
    /// "aging server" degradation that makes parallel optional
    /// connections worthwhile (global rather than per-connection: worker
    /// connection recycling is platform-dependent, and the spawn signal
    /// only needs live connections to underperform their demonstrated
    /// capacity).
    pub degrade_after: Arc<AtomicU32>,
    /// byte ranges (exclusive end) already served per path (for the
    /// inconsistent-edge flip)
    covered: Arc<Mutex<Coverage>>,
    /// total accepted TCP connections (keep-alive reuse verification)
    pub connections_accepted: Arc<AtomicU32>,
    /// requests served per path (mirror weighting verification)
    pub requests_per_path: Arc<Mutex<HashMap<String, u32>>>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TestServer {
    pub fn start() -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
        let flaky = Arc::new(AtomicU32::new(0));
        let slow_ms = Arc::new(AtomicU32::new(0));
        let flip = Arc::new(AtomicBool::new(false));
        let degrade = Arc::new(AtomicU32::new(0));
        let covered: Arc<Mutex<Coverage>> = Arc::new(Mutex::new(HashMap::new()));
        let conns = Arc::new(AtomicU32::new(0));
        let reqs: Arc<Mutex<HashMap<String, u32>>> = Arc::new(Mutex::new(HashMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let files_c = files.clone();
        let flaky_c = flaky.clone();
        let slow_c = slow_ms.clone();
        let flip_c = flip.clone();
        let degrade_c = degrade.clone();
        let covered_c = covered.clone();
        let conns_c = conns.clone();
        let reqs_c = reqs.clone();
        let stop_c = stop.clone();
        listener.set_nonblocking(true).unwrap();
        let handle = std::thread::spawn(move || loop {
            if stop_c.load(Ordering::Relaxed) {
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    conns_c.fetch_add(1, Ordering::SeqCst);
                    let files = files_c.clone();
                    let flaky = flaky_c.clone();
                    let slow = slow_c.clone();
                    let flip = flip_c.clone();
                    let degrade = degrade_c.clone();
                    let covered = covered_c.clone();
                    let reqs = reqs_c.clone();
                    std::thread::spawn(move || {
                        let _ = serve(stream, files, flaky, slow, flip, degrade, covered, reqs);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        });
        TestServer {
            addr,
            files,
            flaky_remaining: flaky,
            slow_ms,
            flip_first_covered_repetition: flip,
            degrade_after: degrade,
            covered,
            connections_accepted: conns,
            requests_per_path: reqs,
            stop,
            handle: Some(handle),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn set_file(&self, path: &str, data: Vec<u8>) {
        self.files.lock().unwrap().insert(path.to_string(), data);
    }

    pub fn set_flaky(&self, n: u32) {
        self.flaky_remaining.store(n, Ordering::SeqCst);
    }

    pub fn set_slow(&self, ms_per_4k: u32) {
        self.slow_ms.store(ms_per_4k, Ordering::SeqCst);
    }

    /// Arm the one-shot inconsistent edge (S11-i B1 e2e).
    pub fn set_inconsistent_edge(&self) {
        self.flip_first_covered_repetition
            .store(true, Ordering::SeqCst);
    }

    /// Degrade responses (apply slow_ms) beyond the first N range requests
    /// for the path, globally (S11-i B2 e2e).
    pub fn set_degrade_after(&self, reqs: u32) {
        self.degrade_after.store(reqs, Ordering::SeqCst);
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn serve(
    mut stream: TcpStream,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    flaky: Arc<AtomicU32>,
    slow_ms: Arc<AtomicU32>,
    flip: Arc<AtomicBool>,
    degrade_after: Arc<AtomicU32>,
    covered: Arc<Mutex<Coverage>>,
    requests_per_path: Arc<Mutex<HashMap<String, u32>>>,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    // support several pipelined requests per connection (keep-alive)
    for _ in 0..1024 {
        let head = match read_head(&mut stream) {
            Some(h) => h,
            None => return Ok(()),
        };
        let mut lines = head.split("\r\n");
        let first = lines.next().unwrap_or("");
        let mut parts = first.split_whitespace();
        let method = parts.next().unwrap_or("");
        let target = parts.next().unwrap_or("/");
        let path = target.split('?').next().unwrap_or("/");
        {
            let mut reqs = requests_per_path.lock().unwrap();
            *reqs.entry(path.to_string()).or_insert(0) += 1;
        }
        let mut range: Option<(u64, Option<u64>)> = None;
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim().eq_ignore_ascii_case("range") {
                    let v = v.trim();
                    if let Some(spec) = v.strip_prefix("bytes=") {
                        let (a, b) = spec.split_once('-').unwrap_or((spec, ""));
                        let start: u64 = a.parse().unwrap_or(0);
                        let end: Option<u64> = if b.is_empty() { None } else { b.parse().ok() };
                        range = Some((start, end));
                    }
                }
            }
        }
        // flaky handling
        if path.starts_with("/flaky") {
            let prev = flaky.try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                if v > 0 {
                    Some(v - 1)
                } else {
                    None
                }
            });
            if let Ok(remaining) = prev {
                if remaining > 0 {
                    write_simple(&mut stream, 503, "Service Unavailable", b"flaky")?;
                    continue;
                }
            }
        }
        let data = files.lock().unwrap().get(path).cloned();
        match data {
            Some(body) => {
                let content_type = if path.ends_with(".m3u8") {
                    "application/vnd.apple.mpegurl"
                } else if path.ends_with(".ts") {
                    "video/mp2t"
                } else if path.ends_with(".html") {
                    "text/html"
                } else {
                    "application/octet-stream"
                };
                let etag = format!("\"{}\"", hex_of(path));
                if method == "HEAD" {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: {}\r\nContent-Type: {}\r\nConnection: keep-alive\r\n\r\n",
                        body.len(),
                        etag,
                        content_type
                    );
                    stream.write_all(resp.as_bytes())?;
                    stream.flush()?;
                    continue;
                }
                match range {
                    Some((start, end)) => {
                        let start = start.min(body.len() as u64) as usize;
                        let end = end
                            .map(|e| (e + 1).min(body.len() as u64) as usize)
                            .unwrap_or(body.len());
                        if start >= end {
                            write_simple(&mut stream, 416, "Range Not Satisfiable", b"range")?;
                            continue;
                        }
                        // S11-i B1: the inconsistent-edge flip — one-shot on
                        // the first range request whose bytes were fully
                        // served before
                        let prior_covered = {
                            let cov = covered.lock().unwrap();
                            cov.get(path)
                                .map(|v| range_covered(v, start as u64, end as u64))
                                .unwrap_or(false)
                        };
                        let flip_this = path.starts_with("/inconsistent")
                            && prior_covered
                            && flip.swap(false, Ordering::SeqCst);
                        {
                            let mut cov = covered.lock().unwrap();
                            cov.entry(path.to_string())
                                .or_default()
                                .push((start as u64, end as u64));
                        }
                        let owned: Option<Vec<u8>> = if flip_this {
                            Some(body[start..end].iter().map(|b| !*b).collect())
                        } else {
                            None
                        };
                        let slice: &[u8] = owned.as_deref().unwrap_or(&body[start..end]);
                        // S11-i B2: aging-server degradation — beyond the
                        // first N requests for this path (incl. the probe
                        // HEAD; counted at parse time above), responses are
                        // slowed globally, regardless of connection
                        let degrade_n = degrade_after.load(Ordering::SeqCst);
                        let path_reqs = {
                            let reqs_map = requests_per_path.lock().unwrap();
                            *reqs_map.get(path).unwrap_or(&0)
                        };
                        let degraded = degrade_n > 0 && path_reqs > degrade_n;
                        let slow = if path.starts_with("/slow") || degraded {
                            slow_ms.load(Ordering::Relaxed)
                        } else {
                            0
                        };
                        let resp = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nETag: {}\r\nContent-Type: {}\r\nConnection: keep-alive\r\n\r\n",
                            slice.len(),
                            start,
                            end - 1,
                            body.len(),
                            etag,
                            content_type
                        );
                        stream.write_all(resp.as_bytes())?;
                        if slow > 0 {
                            for chunk in slice.chunks(4096) {
                                stream.write_all(chunk)?;
                                stream.flush()?;
                                std::thread::sleep(Duration::from_millis(slow as u64));
                            }
                        } else {
                            stream.write_all(slice)?;
                        }
                        stream.flush()?;
                    }
                    None => {
                        let slow = slow_ms.load(Ordering::Relaxed);
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: {}\r\nContent-Type: {}\r\nConnection: keep-alive\r\n\r\n",
                            body.len(),
                            etag,
                            content_type
                        );
                        stream.write_all(resp.as_bytes())?;
                        if path.starts_with("/slow") && slow > 0 {
                            for chunk in body.chunks(4096) {
                                stream.write_all(chunk)?;
                                stream.flush()?;
                                std::thread::sleep(Duration::from_millis(slow as u64));
                            }
                        } else {
                            stream.write_all(&body)?;
                        }
                        stream.flush()?;
                    }
                }
            }
            None => {
                write_simple(&mut stream, 404, "Not Found", b"not found")?;
            }
        }
    }
    Ok(())
}

fn write_simple(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        status,
        reason,
        body.len()
    );
    stream.write_all(resp.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return None,
            Ok(_) => {
                buf.push(byte[0]);
                if buf.ends_with(b"\r\n\r\n") {
                    return Some(String::from_utf8_lossy(&buf).into_owned());
                }
                if buf.len() > 65536 {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
}

fn hex_of(s: &str) -> String {
    let mut h = 0u64;
    for b in s.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    format!("{:016x}", h)
}

/// Whether the recorded served intervals (exclusive ends) already cover
/// `[s, e)` entirely (S11-i B1 inconsistent-edge flip).
fn range_covered(list: &[(u64, u64)], s: u64, e: u64) -> bool {
    let mut sorted: Vec<(u64, u64)> = list.to_vec();
    sorted.sort_unstable();
    let mut reach = s;
    for &(a, b) in sorted.iter() {
        if a > reach {
            break;
        }
        if b > reach {
            reach = b;
        }
        if reach >= e {
            return true;
        }
    }
    reach >= e
}

/// Wait until the predicate holds or the timeout elapses (ms).
pub fn wait_until_ms(timeout_ms: u64, mut pred: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    pred()
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}
