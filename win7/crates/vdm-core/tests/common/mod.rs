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
    /// S11-i B8: per-path Content-Type overrides (the by-extension default
    /// stands when a path is absent here).
    pub types: Arc<Mutex<HashMap<String, String>>>,
    /// paths starting with "/flaky" fail the first N requests globally
    pub flaky_remaining: Arc<AtomicU32>,
    /// ms to sleep per 4 KiB written for paths under "/slow"
    pub slow_ms: Arc<AtomicU32>,
    /// S11-i B1 e2e: when armed, covered-repetition requests whose range
    /// is exactly a 256 KiB-aligned block — the engine-audit request shape;
    /// worker leases are guided and never block-shaped — are served with
    /// flipped bytes. A signed budget of 2 decrements without wrap-around
    /// (an unsigned fetch_sub from 0 wraps to MAX and would re-arm!);
    /// one wasted flip is tolerated, a second detects the inconsistency.
    pub flip_first_covered_repetition: Arc<AtomicBool>,
    /// remaining flip budget (signed; see above)
    flip_budget: Arc<std::sync::atomic::AtomicI32>,
    /// S11-i B2 e2e: when > 0, range requests ENTIRELY BELOW this byte
    /// frontier are degraded (slow_ms applied) — the "aging server".
    /// Worker leases are taken from the pool's high end first, so early
    /// leases sit above the frontier (fast, establishing the capacity
    /// reference) and late leases crawl — deterministic on every platform,
    /// independent of probe fallbacks or request ordering.
    pub degrade_after: Arc<AtomicU32>,
    /// S11-i B17 e2e: the first N ranged responses to "/b17" paths carry a
    /// REAL zlib `Content-Encoding: deflate` body (a stored-block stream of
    /// the same slice — valid zlib, but NOT the raw bytes the range
    /// promises). A correct engine rejects the response and retries; the
    /// final file must still be byte-identical to the source.
    pub compressed_ranged_remaining: Arc<AtomicU32>,
    /// S11-i B18 e2e: when armed, ranged requests with start > 0 to
    /// "/b18.bin" are answered with a 200 full body (the resource-changed
    /// simulation; the pos==0 request stays a clean 206).
    pub b18_return_200: Arc<AtomicBool>,
    /// S11-i B18 e2e: set once a ranged request carrying `If-Range` arrives.
    pub b18_if_range_seen: Arc<AtomicBool>,
    /// S11-i B19 e2e: when armed, HEAD and ranged requests to "/b19.bin"
    /// return 403 (the expired/forbidden-link simulation).
    pub b19_forbid: Arc<AtomicBool>,
    /// S11-i B21b e2e: the first N requests to "/ra.bin" return 503 with
    /// `Retry-After: 2` (the server-controlled-wait simulation); subsequent
    /// requests serve the file normally.
    pub retry_after_remaining: Arc<AtomicU32>,
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
        let types: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let flaky = Arc::new(AtomicU32::new(0));
        let slow_ms = Arc::new(AtomicU32::new(0));
        let flip = Arc::new(AtomicBool::new(false));
        let flip_budget = Arc::new(std::sync::atomic::AtomicI32::new(0));
        let flip_budget_c = flip_budget.clone();
        let degrade = Arc::new(AtomicU32::new(0));
        let b17compress = Arc::new(AtomicU32::new(0));
        let b18ret200 = Arc::new(AtomicBool::new(false));
        let b18seen = Arc::new(AtomicBool::new(false));
        let b19forbid = Arc::new(AtomicBool::new(false));
        let ra_remaining = Arc::new(AtomicU32::new(0));
        let raf_c = ra_remaining.clone();
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
        let types_c = types.clone();
        let b17c = b17compress.clone();
        let b18r = b18ret200.clone();
        let b18s = b18seen.clone();
        let b19f = b19forbid.clone();
        listener.set_nonblocking(true).unwrap();
        let handle = std::thread::spawn(move || loop {
            if stop_c.load(Ordering::Relaxed) {
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    conns_c.fetch_add(1, Ordering::SeqCst);
                    let files = files_c.clone();
                    let types = types_c.clone();
                    let flaky = flaky_c.clone();
                    let slow = slow_c.clone();
                    let flip = flip_c.clone();
                    let flip_budget = flip_budget_c.clone();
                    let degrade = degrade_c.clone();
                    let covered = covered_c.clone();
                    let reqs = reqs_c.clone();
                    let b17c = b17c.clone();
                    let b18r = b18r.clone();
                    let b18s = b18s.clone();
                    let b19f = b19f.clone();
                    let raf = raf_c.clone();
                    std::thread::spawn(move || {
                        let _ = serve(
                            stream,
                            files,
                            types,
                            flaky,
                            slow,
                            flip,
                            flip_budget,
                            degrade,
                            covered,
                            reqs,
                            b17c,
                            b18r,
                            b18s,
                            b19f,
                            raf,
                        );
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
            types,
            flaky_remaining: flaky,
            slow_ms,
            flip_first_covered_repetition: flip,
            flip_budget,
            degrade_after: degrade,
            covered,
            connections_accepted: conns,
            requests_per_path: reqs,
            compressed_ranged_remaining: b17compress,
            b18_return_200: b18ret200,
            b18_if_range_seen: b18seen,
            b19_forbid: b19forbid,
            retry_after_remaining: ra_remaining,
            stop,
            handle: Some(handle),
        }
    }

    /// Arm the S11-i B17 e2e: the first `n` ranged responses to "/b17" paths
    /// carry a real zlib body announced as `Content-Encoding: deflate`.
    pub fn set_compressed_ranged(&self, n: u32) {
        self.compressed_ranged_remaining.store(n, Ordering::SeqCst);
    }

    /// Arm the S11-i B21b e2e: the first `n` requests to "/ra.bin" return
    /// 503 with `Retry-After: 2` before the file is served normally.
    pub fn set_retry_after_once(&self, n: u32) {
        self.retry_after_remaining.store(n, Ordering::SeqCst);
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn set_file(&self, path: &str, data: Vec<u8>) {
        self.files.lock().unwrap().insert(path.to_string(), data);
    }

    /// S11-i B8: serve a path with an explicit Content-Type (overrides the
    /// by-extension default).
    pub fn set_file_with_type(&self, path: &str, data: Vec<u8>, content_type: &str) {
        self.set_file(path, data);
        self.types
            .lock()
            .unwrap()
            .insert(path.to_string(), content_type.to_string());
    }

    pub fn set_flaky(&self, n: u32) {
        self.flaky_remaining.store(n, Ordering::SeqCst);
    }

    pub fn set_slow(&self, ms_per_4k: u32) {
        self.slow_ms.store(ms_per_4k, Ordering::SeqCst);
    }

    /// Arm the inconsistent edge (S11-i B1 e2e): a signed budget of 2
    /// flips, each applying only to covered-repetition requests shaped like
    /// the engine's 256 KiB audit blocks (worker leases never match the
    /// shape, so only audits can consume the budget).
    pub fn set_inconsistent_edge(&self) {
        self.flip_first_covered_repetition
            .store(true, Ordering::SeqCst);
        self.flip_budget.store(2, Ordering::SeqCst);
    }

    /// Degrade (apply slow_ms) to range requests entirely below the byte
    /// frontier (S11-i B2 e2e) — see the field docs.
    pub fn set_degrade_below(&self, frontier_bytes: u32) {
        self.degrade_after.store(frontier_bytes, Ordering::SeqCst);
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
    types: Arc<Mutex<HashMap<String, String>>>,
    flaky: Arc<AtomicU32>,
    slow_ms: Arc<AtomicU32>,
    flip: Arc<AtomicBool>,
    flip_budget: Arc<std::sync::atomic::AtomicI32>,
    degrade_after: Arc<AtomicU32>,
    covered: Arc<Mutex<Coverage>>,
    requests_per_path: Arc<Mutex<HashMap<String, u32>>>,
    b17compress: Arc<AtomicU32>,
    b18_return_200: Arc<AtomicBool>,
    b18_if_range_seen: Arc<AtomicBool>,
    b19_forbid: Arc<AtomicBool>,
    ra_remaining: Arc<AtomicU32>,
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
        let mut if_range_req = false;
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
                // S11-i B18 e2e witness: a ranged request carrying If-Range
                if k.trim().eq_ignore_ascii_case("if-range") {
                    if_range_req = true;
                }
            }
        }
        if range.is_some() && if_range_req {
            b18_if_range_seen.store(true, Ordering::SeqCst);
        }
        // S11-i B19 e2e: expired/forbidden link simulation
        if path == "/b19.bin"
            && b19_forbid.load(Ordering::SeqCst)
            && (method == "HEAD" || range.is_some())
        {
            write_simple(&mut stream, 403, "Forbidden", b"forbidden")?;
            continue;
        }
        // S11-i B21b e2e: a server-controlled throttle — the first N
        // requests get 503 + `Retry-After: 2` (delta-seconds form).
        if path == "/ra.bin" {
            let prev = ra_remaining.try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                if v > 0 {
                    Some(v - 1)
                } else {
                    None
                }
            });
            if prev.is_ok() {
                let resp = "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 2\r\nContent-Length: 9\r\nConnection: keep-alive\r\n\r\nthrottled";
                stream.write_all(resp.as_bytes())?;
                stream.flush()?;
                continue;
            }
        }
        // S11-i B21a e2e: a server that NEVER supports ranges (no
        // Accept-Ranges advertised; ranged GETs answered with a full 200).
        if path.starts_with("/nores/") {
            let body = files.lock().unwrap().get(path).cloned();
            if let Some(body) = body {
                let etag = format!("\"{}\"", hex_of(path));
                if method == "HEAD" {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {}\r\nContent-Type: application/octet-stream\r\nConnection: keep-alive\r\n\r\n",
                        body.len(),
                        etag
                    );
                    stream.write_all(resp.as_bytes())?;
                    stream.flush()?;
                    continue;
                }
                // ranged or not: a full 200 — the engine must classify this
                // server as no-resume from the probe onward.
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {}\r\nContent-Type: application/octet-stream\r\nConnection: keep-alive\r\n\r\n",
                    body.len(),
                    etag
                );
                stream.write_all(resp.as_bytes())?;
                stream.write_all(&body)?;
                stream.flush()?;
                continue;
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
                let content_type = if let Some(t) = types.lock().unwrap().get(path) {
                    t.clone()
                } else if path.ends_with(".m3u8") {
                    "application/vnd.apple.mpegurl".to_string()
                } else if path.ends_with(".ts") {
                    "video/mp2t".to_string()
                } else if path.ends_with(".html") {
                    "text/html".to_string()
                } else {
                    "application/octet-stream".to_string()
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
                        // S11-i B18 e2e: the changed-resource simulation —
                        // ranged requests above byte 0 get a 200 full body.
                        if path == "/b18.bin" && b18_return_200.load(Ordering::SeqCst) && start > 0
                        {
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nETag: {}\r\nContent-Type: {}\r\nConnection: keep-alive\r\n\r\n",
                                body.len(),
                                etag,
                                content_type
                            );
                            stream.write_all(resp.as_bytes())?;
                            stream.write_all(&body)?;
                            stream.flush()?;
                            continue;
                        }
                        let start = start.min(body.len() as u64) as usize;
                        let end = end
                            .map(|e| (e + 1).min(body.len() as u64) as usize)
                            .unwrap_or(body.len());
                        if start >= end {
                            write_simple(&mut stream, 416, "Range Not Satisfiable", b"range")?;
                            continue;
                        }
                        // S11-i B17 e2e: the first budgeted ranged responses
                        // carry a real zlib stream (of the clamped slice)
                        // announced as `Content-Encoding: deflate`.
                        let mut compressed_body: Option<Vec<u8>> = None;
                        if path.starts_with("/b17") {
                            let prev =
                                b17compress.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                                    if n > 0 {
                                        Some(n - 1)
                                    } else {
                                        None
                                    }
                                });
                            if prev.is_ok() {
                                compressed_body = Some(zlib_stored(&body[start..end]));
                            }
                        }
                        // S11-i B1: the inconsistent-edge flip — armed,
                        // covered-repetition, exactly the engine-audit
                        // block shape (256 KiB-aligned), and budget left.
                        // Worker leases are guided (>= 512 KiB) and never
                        // match the shape, so only audits consume flips.
                        // The signed budget decrements without wrapping
                        // below zero (an unsigned fetch_sub from 0 wraps
                        // to MAX and would re-arm the flips forever).
                        #[allow(clippy::manual_is_multiple_of)]
                        fn block_shaped(start: usize, end: usize) -> bool {
                            const AUDIT_BLOCK: usize = 256 * 1024;
                            start % AUDIT_BLOCK == 0 && end - start == AUDIT_BLOCK
                        }
                        let prior_covered = {
                            let cov = covered.lock().unwrap();
                            cov.get(path)
                                .map(|v| range_covered(v, start as u64, end as u64))
                                .unwrap_or(false)
                        };
                        let shaped = block_shaped(start, end);
                        let flip_this = path.starts_with("/inconsistent")
                            && prior_covered
                            && shaped
                            && flip.load(Ordering::SeqCst)
                            && flip_budget.fetch_sub(1, Ordering::SeqCst) > 0;
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
                        // S11-i B2: aging-server degradation — ranges
                        // entirely below the byte frontier crawl. Leases
                        // come from the pool's high end first, so early
                        // leases are fast (capacity reference) and late
                        // leases crawl — deterministic on every platform.
                        let frontier = degrade_after.load(Ordering::SeqCst);
                        let degraded = frontier > 0 && (end as u32) <= frontier;
                        let slow = if path.starts_with("/slow") || degraded {
                            slow_ms.load(Ordering::Relaxed)
                        } else {
                            0
                        };
                        let out_slice: &[u8] = compressed_body.as_deref().unwrap_or(slice);
                        let ce_line = if compressed_body.is_some() {
                            "Content-Encoding: deflate\r\n"
                        } else {
                            ""
                        };
                        let resp = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n{}Content-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nETag: {}\r\nContent-Type: {}\r\nConnection: keep-alive\r\n\r\n",
                            out_slice.len(),
                            ce_line,
                            start,
                            end - 1,
                            body.len(),
                            etag,
                            content_type
                        );
                        stream.write_all(resp.as_bytes())?;
                        if slow > 0 {
                            for chunk in out_slice.chunks(4096) {
                                stream.write_all(chunk)?;
                                stream.flush()?;
                                std::thread::sleep(Duration::from_millis(slow as u64));
                            }
                        } else {
                            stream.write_all(out_slice)?;
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

/// Wrap `data` in a zlib stream built from stored (uncompressed) deflate
/// blocks — valid zlib that decompresses to `data`, with wire bytes that
/// differ from `data` (S11-i B17 e2e discriminator). Hand-rolled from
/// RFC 1950/1951 (the tests must not depend on a compression crate).
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 16);
    out.push(0x78); // CMF: deflate, 32K window
    out.push(0x01); // FLG: check bits valid, no dict, fastest
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    } else {
        let last = data.chunks(65535).count() - 1;
        for (i, chunk) in data.chunks(65535).enumerate() {
            out.push(if i == last { 0x01 } else { 0x00 }); // BFINAL+BTYPE=00
            let len = chunk.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(chunk);
        }
    }
    // Adler-32 trailer (RFC 1950)
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
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
