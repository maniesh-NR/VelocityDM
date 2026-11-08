// SPDX-License-Identifier: GPL-3.0-or-later
//! Minimal HTTP/1.1 client with a keep-alive connection pool, written for the
//! engine's needs: GET/HEAD, Range requests, redirect following, bounded
//! head parsing, and chunked/content-length/EOF body decoding.
//!
//! Design notes:
//! - GET and HEAD requests carry no body, which removes the entire class of
//!   "request body polluted the pooled connection" bugs by construction.
//! - A pooled connection is only returned to the pool after its response body
//!   has been consumed to a clean boundary; otherwise the socket is dropped.
//! - Reads are wrapped in an IO timeout so a stalled server cannot hang a
//!   worker forever.
//! - The IO staging buffer lives on the heap inside `Body` (not in the async
//!   fn), so composed futures stay small even for inline callers.

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use url::Url;

use crate::error::{Result, VdmError};

// ---------------------------------------------------------------------------
// Transport stream
// ---------------------------------------------------------------------------

pub enum Stream {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl Unpin for Stream {}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.as_mut().get_mut();
        match this {
            Stream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.as_mut().get_mut();
        match this {
            Stream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        let this = self.as_mut().get_mut();
        match this {
            Stream::Plain(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.as_mut().get_mut();
        match this {
            Stream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

// ---------------------------------------------------------------------------
// Connection pool
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Pool {
    shared: Arc<PoolShared>,
}

struct PoolShared {
    hosts: Mutex<HashMap<String, VecDeque<Stream>>>,
    max_per_host: Mutex<usize>,
    connects: AtomicU64,
    reuses: AtomicU64,
}

impl Pool {
    pub fn new(max_per_host: usize) -> Self {
        Pool {
            shared: Arc::new(PoolShared {
                hosts: Mutex::new(HashMap::new()),
                max_per_host: Mutex::new(max_per_host.max(1)),
                connects: AtomicU64::new(0),
                reuses: AtomicU64::new(0),
            }),
        }
    }

    pub fn set_max_per_host(&self, n: usize) {
        *self.shared.max_per_host.lock().unwrap() = n.max(1);
    }

    pub fn max_per_host(&self) -> usize {
        *self.shared.max_per_host.lock().unwrap()
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.shared.connects.load(Ordering::Relaxed), self.shared.reuses.load(Ordering::Relaxed))
    }

    fn take(&self, key: &str) -> Option<Stream> {
        let mut hosts = self.shared.hosts.lock().unwrap();
        let entry = hosts.get_mut(key)?;
        let stream = entry.pop_back()?;
        self.shared.reuses.fetch_add(1, Ordering::Relaxed);
        Some(stream)
    }

    fn give(&self, key: &str, stream: Stream) {
        let max = *self.shared.max_per_host.lock().unwrap();
        let mut hosts = self.shared.hosts.lock().unwrap();
        let entry = hosts.entry(key.to_string()).or_default();
        if entry.len() < max {
            entry.push_back(stream);
        }
        // else: drop the stream (socket closed).
    }

    pub fn idle_count(&self) -> usize {
        self.shared.hosts.lock().unwrap().values().map(|v| v.len()).sum()
    }
}

async fn connect_stream(url: &Url, pool: &Pool, connect_timeout: Duration) -> Result<Stream> {
    let host = url.host_str().ok_or_else(|| VdmError::Url("missing host".into()))?.to_string();
    let port = url.port_or_known_default().unwrap_or(80);
    let scheme = url.scheme();
    pool.shared.connects.fetch_add(1, Ordering::Relaxed);

    let tcp = tokio::time::timeout(connect_timeout, TcpStream::connect((host.as_str(), port)))
        .await
        .map_err(|_| VdmError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timeout")))??;
    tcp.set_nodelay(true).ok();

    match scheme {
        "http" => Ok(Stream::Plain(tcp)),
        "https" => {
            #[cfg(feature = "tls")]
            {
                let mut roots = rustls::RootCertStore::empty();
                roots.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
                    rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
                        ta.subject.to_vec(),
                        ta.spki.to_vec(),
                        ta.name_constraints.map(|nc| nc.to_vec()),
                    )
                }));
                let config = Arc::new(
                    rustls::ClientConfig::builder()
                        .with_safe_defaults()
                        .with_root_certificates(roots)
                        .with_no_client_auth(),
                );
                let server_name = rustls::ServerName::try_from(host.as_str())
                    .map_err(|e| VdmError::Url(format!("bad tls server name: {}", e)))?;
                let connector = tokio_rustls::TlsConnector::from(config);
                let tls = tokio::time::timeout(connect_timeout, connector.connect(server_name, tcp))
                    .await
                    .map_err(|_| VdmError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "tls handshake timeout")))?
                    .map_err(|e| VdmError::Http(format!("tls handshake: {}", e)))?;
                Ok(Stream::Tls(Box::new(tls)))
            }
            #[cfg(not(feature = "tls"))]
            {
                let _ = url;
                Err(VdmError::Unsupported("https requires the 'tls' feature".into()))
            }
        }
        other => Err(VdmError::Url(format!("unsupported scheme: {}", other))),
    }
}

// ---------------------------------------------------------------------------
// Requests and responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
        }
    }
}

#[derive(Clone)]
pub struct HttpClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    pool: Pool,
    user_agent: String,
    connect_timeout: Duration,
    io_timeout: Duration,
    max_redirects: usize,
}

impl Default for HttpClient {
    fn default() -> Self {
        HttpClient::new("VelocityDM")
    }
}

impl HttpClient {
    pub fn new(user_agent: impl Into<String>) -> Self {
        HttpClient {
            inner: Arc::new(ClientInner {
                pool: Pool::new(8),
                user_agent: user_agent.into(),
                connect_timeout: Duration::from_secs(10),
                io_timeout: Duration::from_secs(30),
                max_redirects: 5,
            }),
        }
    }

    pub fn pool(&self) -> &Pool {
        &self.inner.pool
    }

    pub fn user_agent(&self) -> &str {
        &self.inner.user_agent
    }

    /// Performs a GET/HEAD with redirect following. `range` is an inclusive
    /// byte range `(start, Some(end))` or an open range `(start, None)`.
    pub async fn request(
        &self,
        method: Method,
        url: &str,
        headers: &[(String, String)],
        range: Option<(u64, Option<u64>)>,
    ) -> Result<Response> {
        let mut current = Url::parse(url)?;
        let mut redirects = 0usize;
        loop {
            let (status, mut resp) = self.single_request(method, &current, headers, range).await?;
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                if redirects >= self.inner.max_redirects {
                    return Err(VdmError::Http("too many redirects".into()));
                }
                let location = match resp.headers.get("location") {
                    Some(l) => l.clone(),
                    None => return Ok(resp),
                };
                let next = current.join(&location)?;
                redirects += 1;
                current = next;
                continue;
            }
            resp.final_url = current.to_string();
            return Ok(resp);
        }
    }

    pub async fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Response> {
        self.request(Method::Get, url, headers, None).await
    }

    pub async fn head(&self, url: &str, headers: &[(String, String)]) -> Result<Response> {
        self.request(Method::Head, url, headers, None).await
    }

    async fn single_request(
        &self,
        method: Method,
        url: &Url,
        headers: &[(String, String)],
        range: Option<(u64, Option<u64>)>,
    ) -> Result<(u16, Response)> {
        let host = url.host_str().ok_or_else(|| VdmError::Url("missing host".into()))?.to_string();
        let port = url.port_or_known_default().unwrap_or(80);
        let pool_key = format!("{}:{}:{}", url.scheme(), host, port);

        // Reuse a pooled connection when possible; on any write failure fall
        // back to a fresh connection once (stale pooled sockets).
        let mut last_err: Option<VdmError> = None;
        for attempt in 0..2 {
            let stream = if attempt == 0 {
                match self.inner.pool.take(&pool_key) {
                    Some(s) => s,
                    None => connect_stream(url, &self.inner.pool, self.inner.connect_timeout).await?,
                }
            } else {
                connect_stream(url, &self.inner.pool, self.inner.connect_timeout).await?
            };
            let mut stream = stream;
            let request_line = build_request(method, url, &host, port, &self.inner.user_agent, headers, range);
            let write_res = async {
                stream.write_all(request_line.as_bytes()).await?;
                stream.flush().await?;
                Ok::<(), VdmError>(())
            }
            .await;
            if let Err(e) = write_res {
                last_err = Some(e);
                continue;
            }
            match read_response_head(&mut stream, self.inner.io_timeout).await {
                Ok((status, header_map, leftover)) => {
                    let body = Body::new(
                        stream,
                        self.inner.pool.clone(),
                        &pool_key,
                        method,
                        status,
                        &header_map,
                        leftover,
                        self.inner.io_timeout,
                    );
                    let resp = Response { status, headers: header_map, body, final_url: String::new() };
                    return Ok((status, resp));
                }
                Err(e) => {
                    // If reading the head failed the connection is unusable.
                    last_err = Some(e);
                    continue;
                }
            }
        }
        Err(last_err.unwrap_or_else(|| VdmError::Http("request failed".into())))
    }
}

fn build_request(
    method: Method,
    url: &Url,
    host: &str,
    port: u16,
    user_agent: &str,
    headers: &[(String, String)],
    range: Option<(u64, Option<u64>)>,
) -> String {
    let path = match (url.path(), url.query()) {
        (p, Some(q)) => format!("{}?{}", p, q),
        (p, None) => p.to_string(),
    };
    let host_header = if url.scheme() == "http" && port == 80 || url.scheme() == "https" && port == 443 {
        host.to_string()
    } else {
        format!("{}:{}", host, port)
    };
    let mut req = String::with_capacity(256);
    req.push_str(method.as_str());
    req.push(' ');
    req.push_str(&path);
    req.push_str(" HTTP/1.1\r\n");
    req.push_str(&format!("Host: {}\r\n", host_header));
    req.push_str(&format!("User-Agent: {}\r\n", user_agent));
    req.push_str("Accept: */*\r\n");
    req.push_str("Accept-Encoding: identity\r\n");
    req.push_str("Connection: keep-alive\r\n");
    if let Some((start, end)) = range {
        let end_part = end.map(|v| v.to_string()).unwrap_or_default();
        req.push_str(&format!("Range: bytes={}-{}\r\n", start, end_part));
    }
    for (k, v) in headers {
        let key = k.trim();
        // Never allow custom headers to override framing headers; GET/HEAD
        // requests carry no body, so content-length cannot be injected here.
        let lower = key.to_ascii_lowercase();
        if matches!(lower.as_str(), "host" | "content-length" | "connection" | "transfer-encoding") {
            continue;
        }
        req.push_str(&format!("{}: {}\r\n", key, v.trim()));
    }
    req.push_str("\r\n");
    req
}

fn parse_head(buf: &[u8]) -> Option<(u16, HashMap<String, String>, usize)> {
    let needle: &[u8] = b"\r\n\r\n";
    let end = buf
        .windows(4)
        .position(|w| w == needle)
        .map(|p| p + 4)?;
    let head = std::str::from_utf8(&buf[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next()?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/") {
        return None;
    }
    let status: u16 = parts.next()?.parse().ok()?;
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_ascii_lowercase();
            let value = line[colon + 1..].trim().to_string();
            headers.insert(key, value);
        }
    }
    Some((status, headers, end))
}

async fn read_response_head(stream: &mut Stream, io_timeout: Duration) -> Result<(u16, HashMap<String, String>, Vec<u8>)> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut tmp = [0u8; 4096];
    loop {
        if let Some((status, headers, consumed)) = parse_head(&buf) {
            let leftover = buf[consumed..].to_vec();
            return Ok((status, headers, leftover));
        }
        if buf.len() > 128 * 1024 {
            return Err(VdmError::Http("response head exceeds 128 KiB".into()));
        }
        let n = tokio::time::timeout(io_timeout, stream.read(&mut tmp))
            .await
            .map_err(|_| VdmError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "head read timeout")))?
            .map_err(VdmError::Io)?;
        if n == 0 {
            return Err(VdmError::Http("connection closed before response head completed".into()));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

enum BodyMode {
    Empty,
    Length(u64),
    Chunked,
    Eof,
}

struct Release {
    pool: Pool,
    key: String,
}

pub struct Body {
    stream: Option<Stream>,
    mode: BodyMode,
    buf: Vec<u8>,
    /// Reusable IO staging buffer (heap, allocated once per response) so the
    /// read future stays small; large stack arrays in async fns compose into
    /// oversized futures for inline callers.
    io_buf: Vec<u8>,
    pos: usize,
    chunk_remaining: u64,
    trailing_crlf: u8,
    done: bool,
    reusable: bool,
    io_timeout: Duration,
    release: Option<Release>,
}

impl Body {
    #[allow(clippy::too_many_arguments)]
    fn new(
        stream: Stream,
        pool: Pool,
        pool_key: &str,
        method: Method,
        status: u16,
        headers: &HashMap<String, String>,
        leftover: Vec<u8>,
        io_timeout: Duration,
    ) -> Body {
        let no_body = method == Method::Head || status == 204 || status == 304 || (100..200).contains(&status);
        let mode = if no_body {
            BodyMode::Empty
        } else if headers
            .get("transfer-encoding")
            .map(|v| v.to_ascii_lowercase().contains("chunked"))
            .unwrap_or(false)
        {
            BodyMode::Chunked
        } else if let Some(len) = headers.get("content-length").and_then(|v| v.trim().parse::<u64>().ok()) {
            if len == 0 {
                BodyMode::Empty
            } else {
                BodyMode::Length(len)
            }
        } else {
            BodyMode::Eof
        };
        let reusable_empty = matches!(mode, BodyMode::Empty);
        Body {
            stream: Some(stream),
            mode,
            buf: leftover,
            io_buf: vec![0u8; 64 * 1024],
            pos: 0,
            chunk_remaining: 0,
            trailing_crlf: 0,
            done: reusable_empty,
            reusable: reusable_empty,
            io_timeout,
            release: Some(Release { pool, key: pool_key.to_string() }),
        }
    }

    async fn fill(&mut self) -> std::io::Result<usize> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "body stream already consumed")
        })?;
        self.io_buf.resize(64 * 1024, 0);
        let n = tokio::time::timeout(self.io_timeout, stream.read(&mut self.io_buf))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "body read timeout"))??;
        if n > 0 {
            self.buf.extend_from_slice(&self.io_buf[..n]);
        }
        Ok(n)
    }

    async fn read_line_crlf(&mut self) -> std::io::Result<String> {
        loop {
            if let Some(pos) = find_crlf(&self.buf[self.pos..]) {
                let line = String::from_utf8_lossy(&self.buf[self.pos..self.pos + pos]).to_string();
                self.pos += pos + 2;
                return Ok(line);
            }
            let n = self.fill().await?;
            if n == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "chunked stream truncated"));
            }
        }
    }

    pub async fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.done || out.is_empty() {
            return Ok(0);
        }
        match self.mode {
            BodyMode::Empty => {
                self.done = true;
                self.reusable = true;
                Ok(0)
            }
            BodyMode::Length(remaining) => {
                if remaining == 0 {
                    self.done = true;
                    self.reusable = true;
                    return Ok(0);
                }
                while self.pos == self.buf.len() {
                    let n = self.fill().await?;
                    if n == 0 {
                        self.done = true;
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "connection closed before content-length satisfied",
                        ));
                    }
                }
                let avail = self.buf.len() - self.pos;
                let want = (out.len() as u64).min(remaining).min(avail as u64) as usize;
                out[..want].copy_from_slice(&self.buf[self.pos..self.pos + want]);
                self.pos += want;
                if self.pos == self.buf.len() {
                    self.buf.clear();
                    self.pos = 0;
                }
                let new_remaining = remaining - want as u64;
                if new_remaining == 0 {
                    self.done = true;
                    self.reusable = true;
                    self.mode = BodyMode::Length(0);
                } else {
                    self.mode = BodyMode::Length(new_remaining);
                }
                Ok(want)
            }
            BodyMode::Chunked => {
                loop {
                    if self.trailing_crlf == 1 {
                        // consume the CRLF that terminates the previous chunk
                        let line = self.read_line_crlf().await?;
                        debug_assert!(line.is_empty(), "expected CRLF, got {:?}", line);
                        self.trailing_crlf = 0;
                    }
                    if self.chunk_remaining == 0 {
                        let line = self.read_line_crlf().await?;
                        let size_part = line.split(';').next().unwrap_or("").trim();
                        let size = u64::from_str_radix(size_part, 16).map_err(|_| {
                            std::io::Error::new(std::io::ErrorKind::InvalidData, "bad chunk size")
                        })?;
                        if size == 0 {
                            // trailers until empty line
                            loop {
                                let trailer = self.read_line_crlf().await?;
                                if trailer.is_empty() {
                                    break;
                                }
                            }
                            self.done = true;
                            self.reusable = true;
                            return Ok(0);
                        }
                        self.chunk_remaining = size;
                    }
                    if self.pos == self.buf.len() {
                        let n = self.fill().await?;
                        if n == 0 {
                            self.done = true;
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "connection closed inside a chunk",
                            ));
                        }
                    }
                    let avail = self.buf.len() - self.pos;
                    let want = (out.len() as u64).min(self.chunk_remaining).min(avail as u64) as usize;
                    out[..want].copy_from_slice(&self.buf[self.pos..self.pos + want]);
                    self.pos += want;
                    self.chunk_remaining -= want as u64;
                    if self.pos == self.buf.len() {
                        self.buf.clear();
                        self.pos = 0;
                    }
                    if self.chunk_remaining == 0 {
                        self.trailing_crlf = 1;
                    }
                    if want > 0 {
                        return Ok(want);
                    }
                }
            }
            BodyMode::Eof => {
                if self.pos == self.buf.len() {
                    let n = self.fill().await?;
                    if n == 0 {
                        self.done = true;
                        // EOF bodies leave the connection unusable.
                        self.reusable = false;
                        return Ok(0);
                    }
                }
                let avail = self.buf.len() - self.pos;
                let want = out.len().min(avail);
                out[..want].copy_from_slice(&self.buf[self.pos..self.pos + want]);
                self.pos += want;
                if self.pos == self.buf.len() {
                    self.buf.clear();
                    self.pos = 0;
                }
                Ok(want)
            }
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Reads the remaining body into memory (bounded), for small payloads.
    pub async fn read_to_end_limited(&mut self, limit: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let n = self.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            if out.len() + n > limit {
                return Err(VdmError::Http(format!("body exceeds {} byte limit", limit)));
            }
            out.extend_from_slice(&chunk[..n]);
        }
        Ok(out)
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        if self.reusable {
            if let (Some(stream), Some(release)) = (self.stream.take(), self.release.take()) {
                release.pool.give(&release.key, stream);
            }
        }
        // Otherwise the stream drops and the socket closes.
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Body,
    pub final_url: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(|s| s.as_str())
    }

    pub fn content_length(&self) -> Option<u64> {
        self.headers.get("content-length").and_then(|v| v.trim().parse().ok())
    }

    pub fn final_url(&self) -> &str {
        &self.final_url
    }

    /// Reads the whole body (bounded) and returns it with the response.
    pub async fn read_to_end_limited(mut self, limit: usize) -> Result<(Self, Vec<u8>)> {
        let data = self.body.read_to_end_limited(limit).await?;
        Ok((self, data))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_basic_get() {
        let url = Url::parse("http://example.com/dir/file.zip?a=1").unwrap();
        let req = build_request(Method::Get, &url, "example.com", 80, "VelocityDM/1.0", &[], None);
        assert!(req.starts_with("GET /dir/file.zip?a=1 HTTP/1.1\r\n"));
        assert!(req.contains("Host: example.com\r\n"));
        assert!(req.ends_with("\r\n\r\n"));
        assert!(!req.contains("Content-Length"), "GET must not carry a body");
    }

    #[test]
    fn build_request_head_has_no_body_and_custom_headers() {
        let url = Url::parse("https://example.com:8443/x").unwrap();
        let headers = vec![("X-Test".to_string(), "1".to_string()), ("host".to_string(), "evil".to_string())];
        let req = build_request(Method::Head, &url, "example.com", 8443, "UA", &headers, Some((5, Some(9))));
        assert!(req.starts_with("HEAD /x HTTP/1.1\r\n"));
        assert!(req.contains("Host: example.com:8443\r\n"));
        assert!(req.contains("X-Test: 1\r\n"));
        assert!(!req.contains("evil"), "framing headers cannot be overridden");
        assert!(req.contains("Range: bytes=5-9\r\n"));
    }

    #[test]
    fn build_request_open_range() {
        let url = Url::parse("http://example.com/f").unwrap();
        let req = build_request(Method::Get, &url, "example.com", 80, "UA", &[], Some((128, None)));
        assert!(req.contains("Range: bytes=128-\r\n"));
    }

    #[test]
    fn parse_head_handles_split_packets() {
        let full = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/100\r\n\r\nhello";
        let (a, b) = full.split_at(17);
        let mut joined = Vec::new();
        joined.extend_from_slice(a);
        assert!(parse_head(&joined).is_none(), "incomplete head must not parse");
        joined.extend_from_slice(b);
        let (status, headers, consumed) = parse_head(&joined).unwrap();
        assert_eq!(status, 206);
        assert_eq!(headers["content-range"], "bytes 0-4/100");
        assert_eq!(&joined[consumed..], b"hello");
    }

    #[test]
    fn parse_head_rejects_garbage() {
        assert!(parse_head(b"not http\r\n\r\n").is_none());
        assert!(parse_head(b"").is_none());
    }
}
