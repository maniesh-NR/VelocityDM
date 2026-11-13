//! Minimal blocking HTTP/1.1 client used by the engine.
//!
//! Design goals (clean-room, zero heavy deps):
//! - One `Conn` owns a keep-alive connection; `request()` moves the transport
//!   into the returned `Response`; once the body is drained `into_conn()`
//!   recycles it. Any bytes buffered past the head terminator become an
//!   explicit body prelude that is served through the same accounting path as
//!   socket bytes (never swallowed, never double-counted).
//! - Transfer-Encoding: chunked and Content-Length bodies both supported.
//! - TLS via rustls (`tls` feature), plain TCP otherwise.
//! - Optional HTTP CONNECT / SOCKS5 proxy (see [`crate::proxy`]).

use crate::error::{Error, Result};
use crate::proxy::{ProxyConfig, ProxyStream};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// path + optional query, always starts with `/`.
    pub path_and_query: String,
}

impl Url {
    pub fn parse(url: &str) -> Result<Url> {
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| Error::Http(format!("URL has no scheme: {}", url)))?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(Error::Http(format!("unsupported scheme '{}'", scheme)));
        }
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (
                h.to_string(),
                p.parse::<u16>()
                    .map_err(|_| Error::Http("bad port".into()))?,
            ),
            _ => (
                authority.to_string(),
                if scheme == "https" { 443u16 } else { 80u16 },
            ),
        };
        if host.is_empty() {
            return Err(Error::Http("empty host".into()));
        }
        Ok(Url {
            scheme,
            host,
            port,
            path_and_query: path.to_string(),
        })
    }

    pub fn is_tls(&self) -> bool {
        self.scheme == "https"
    }

    pub fn host_header(&self) -> String {
        let default = if self.is_tls() { 443 } else { 80 };
        if self.port == default {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn join(&self, location: &str) -> Result<Url> {
        if location.contains("://") {
            return Url::parse(location);
        }
        if location.starts_with('/') {
            return Ok(Url {
                scheme: self.scheme.clone(),
                host: self.host.clone(),
                port: self.port,
                path_and_query: normalize_path_and_query(location),
            });
        }
        let base = self.path_and_query.split(['?', '#']).next().unwrap_or("/");
        let dir = match base.rfind('/') {
            Some(i) => &base[..=i],
            None => "/",
        };
        Ok(Url {
            scheme: self.scheme.clone(),
            host: self.host.clone(),
            port: self.port,
            path_and_query: normalize_path_and_query(&format!("{}{}", dir, location)),
        })
    }

    pub fn to_string_full(&self) -> String {
        format!(
            "{}://{}{}",
            self.scheme,
            self.host_header(),
            self.path_and_query
        )
    }
}

/// RFC 3986 section 5.2.4 remove_dot_segments, applied to a path+query.
/// `../key.bin` relative URIs are common in HLS playlists; without
/// normalization the request path is sent with literal `..` segments and
/// servers resolve it outside the intended directory.
fn normalize_path_and_query(input: &str) -> String {
    let (path, suffix) = match input.find(['?', '#']) {
        Some(i) => (&input[..i], &input[i..]),
        None => (input, ""),
    };
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let mut result = out.join("/");
    if path.ends_with("/.") {
        result.push('/');
    }
    if path.ends_with("/..") {
        result.push('/');
    }
    if !result.starts_with('/') {
        result.insert(0, '/');
    }
    format!("{}{}", result, suffix)
}

impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_full())
    }
}

/// Client configuration shared by all engine connections.
#[derive(Debug, Clone, Default)]
pub struct ClientCfg {
    pub user_agent: String,
    pub extra_headers: Vec<(String, String)>,
    pub proxy: Option<ProxyConfig>,
    pub connect_timeout: Option<Duration>,
    pub io_timeout: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl Request {
    pub fn get(url: &Url) -> Request {
        Request {
            method: "GET".into(),
            url: url.clone(),
            headers: Vec::new(),
            body: None,
        }
    }
    pub fn head(url: &Url) -> Request {
        Request {
            method: "HEAD".into(),
            url: url.clone(),
            headers: Vec::new(),
            body: None,
        }
    }
    pub fn header(mut self, k: &str, v: &str) -> Request {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }
}

enum Transport {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<crate::tls::TlsStream>),
}

impl Read for Transport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Transport::Plain(s) => s.read(buf),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => s.read(buf),
        }
    }
}
impl Write for Transport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Transport::Plain(s) => s.write(buf),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Transport::Plain(s) => s.flush(),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => s.flush(),
        }
    }
}

/// A single connected transport (plain or TLS), reusable across requests.
pub struct Conn {
    tr: Option<Transport>,
    /// Talking to a plain-HTTP origin *through* an HTTP proxy: requests use
    /// absolute-form URLs and carry Proxy-Authorization.
    absolute_form: bool,
    proxy_auth: Option<String>,
    /// Set once the transport errored; prevents reuse of a poisoned conn.
    poisoned: bool,
}

impl Conn {
    /// Open a connection to the URL origin (honoring proxy + TLS).
    pub fn connect(url: &Url, cfg: &ClientCfg) -> Result<Conn> {
        let ct = cfg.connect_timeout.unwrap_or(Duration::from_secs(15));
        let mut absolute_form = false;
        let mut proxy_auth = None;
        let tcp: TcpStream = match &cfg.proxy {
            None => TcpStream::connect((url.host.as_str(), url.port))
                .map_err(|e| Error::Http(format!("connect {}: {}", url.host, e)))?,
            Some(p) if matches!(p, ProxyConfig::Http { .. }) => {
                let stream = ProxyStream::connect_tcp(p, ct)?;
                if url.is_tls() {
                    let tunnel = ProxyStream::http_connect(p, stream, &url.host, url.port, ct)?;
                    return Ok(Conn::tls_conn(tunnel.into_tcp(), &url.host, false, None));
                }
                absolute_form = true;
                proxy_auth = p.basic_auth();
                stream.into_tcp()
            }
            Some(p) => {
                let stream = ProxyStream::socks_connect(p, &url.host, url.port, ct)?;
                if url.is_tls() {
                    return Ok(Conn::tls_conn(stream.into_tcp(), &url.host, false, None));
                }
                stream.into_tcp()
            }
        };
        if url.is_tls() {
            Ok(Conn::tls_conn(tcp, &url.host, absolute_form, proxy_auth))
        } else {
            Self::apply_timeout(&tcp, cfg);
            Ok(Conn {
                tr: Some(Transport::Plain(tcp)),
                absolute_form,
                proxy_auth,
                poisoned: false,
            })
        }
    }

    #[cfg(feature = "tls")]
    fn tls_conn(
        tcp: TcpStream,
        host: &str,
        absolute_form: bool,
        proxy_auth: Option<String>,
    ) -> Conn {
        match crate::tls::wrap(tcp, host) {
            Ok(t) => Conn {
                tr: Some(Transport::Tls(Box::new(t))),
                absolute_form,
                proxy_auth,
                poisoned: false,
            },
            Err(e) => Conn {
                tr: None,
                absolute_form,
                proxy_auth,
                poisoned: true,
            }
            .with_error(e),
        }
    }

    #[cfg(not(feature = "tls"))]
    fn tls_conn(_tcp: TcpStream, _host: &str, _af: bool, _pa: Option<String>) -> Conn {
        Conn {
            tr: None,
            absolute_form: false,
            proxy_auth: None,
            poisoned: true,
        }
        .with_error(Error::Http("TLS support disabled at compile time".into()))
    }

    fn with_error(mut self, _e: Error) -> Conn {
        // transport error recorded; `request()` will surface it
        self.poisoned = true;
        self
    }

    fn apply_timeout(tcp: &TcpStream, cfg: &ClientCfg) {
        if let Some(t) = cfg.io_timeout {
            let _ = tcp.set_read_timeout(Some(t));
            let _ = tcp.set_write_timeout(Some(t));
        }
    }

    fn transport_error() -> Error {
        Error::Http("connection unavailable (TLS init failed or conn consumed)".into())
    }

    /// Issue a request. The transport moves into the returned `Response`;
    /// `into_conn()` gives it back for keep-alive reuse.
    pub fn request(&mut self, req: &Request, cfg: &ClientCfg) -> Result<Response> {
        if self.tr.is_none() || self.poisoned {
            return Err(Self::transport_error());
        }
        let mut head = String::with_capacity(512);
        let target = if self.absolute_form {
            req.url.to_string_full()
        } else {
            req.url.path_and_query.clone()
        };
        // Same refusal as headers below, applied to the request line and the
        // Host line: the URL is attacker-influenced on tracker announce paths
        // (torrent metadata), and a CRLF there would split the request.
        let bad_char = |s: &str| s.contains('\r') || s.contains('\n') || s.contains('\0');
        if bad_char(&target) || bad_char(&req.url.host_header()) {
            return Err(Error::Http("illegal request target or host".into()));
        }
        head.push_str(&format!("{} {} HTTP/1.1\r\n", req.method, target));
        if !self.absolute_form {
            head.push_str(&format!("Host: {}\r\n", req.url.host_header()));
        } else if let Some(auth) = &self.proxy_auth {
            head.push_str(&format!("Proxy-Authorization: {}\r\n", auth));
        }
        // Header values come from RPC callers (downloads.add options.headers,
        // referer/cookies forwarded from the browser, settings user-agent).
        // Refuse anything that could split the request: CR/LF/NUL in names
        // or values, and non-token header names.
        let push_header = |head: &mut String, k: &str, v: &str| -> Result<()> {
            let bad_char = |s: &str| s.contains('\r') || s.contains('\n') || s.contains('\0');
            if k.is_empty() || bad_char(k) || bad_char(v) || k.contains(' ') || k.contains(':') {
                return Err(Error::Http(format!(
                    "illegal header name/value: {:?}: {:?}",
                    k, v
                )));
            }
            head.push_str(&format!("{}: {}\r\n", k, v));
            Ok(())
        };
        let ua = if cfg.user_agent.is_empty() {
            "VelocityDM/1.0".to_string()
        } else {
            cfg.user_agent.clone()
        };
        push_header(&mut head, "User-Agent", &ua)?;
        for (k, v) in &cfg.extra_headers {
            push_header(&mut head, k, v)?;
        }
        for (k, v) in &req.headers {
            push_header(&mut head, k, v)?;
        }
        if req.body.is_some() {
            head.push_str(&format!(
                "Content-Length: {}\r\n",
                req.body.as_ref().map(|b| b.len()).unwrap_or(0)
            ));
        }
        head.push_str("Connection: keep-alive\r\n\r\n");

        let mut bytes = head.into_bytes();
        if let Some(b) = &req.body {
            bytes.extend_from_slice(b);
        }

        let mut tr = self.tr.take().expect("checked above");
        let write_res = tr.write_all(&bytes).and_then(|_| tr.flush());
        if let Err(e) = write_res {
            self.poisoned = true;
            return Err(Error::Http(format!("write failed: {}", e)));
        }

        // Read the response head; bytes past the terminator become the prelude.
        let mut raw: Vec<u8> = Vec::with_capacity(2048);
        let mut chunk = [0u8; 4096];
        let head_end;
        loop {
            let n = match tr.read(&mut chunk) {
                Ok(n) => n,
                Err(e) => {
                    self.poisoned = true;
                    return Err(Error::Http(format!("read head failed: {}", e)));
                }
            };
            if n == 0 {
                self.poisoned = true;
                return Err(Error::Http("connection closed before response head".into()));
            }
            let prev = raw.len();
            raw.extend_from_slice(&chunk[..n]);
            if let Some(pos) = find_terminator(&raw, prev.saturating_sub(4)) {
                head_end = pos + 4;
                break;
            }
            if raw.len() > 128 * 1024 {
                self.poisoned = true;
                return Err(Error::Http("response head too large".into()));
            }
        }
        let prelude = raw.split_off(head_end);
        let (status, reason, headers) = match parse_head(&raw) {
            Ok(v) => v,
            Err(e) => {
                self.poisoned = true;
                return Err(e);
            }
        };
        let keep_alive = header_value(&headers, "connection")
            .map(|v| !v.eq_ignore_ascii_case("close"))
            .unwrap_or(true);

        let mut body = BodyState::new(prelude);
        let method_head = req.method.eq_ignore_ascii_case("HEAD");
        body.init_mode(&headers, method_head);
        Ok(Response {
            status,
            reason,
            headers,
            body,
            keep_alive,
            tr: Some(tr),
            absolute_form: self.absolute_form,
            proxy_auth: self.proxy_auth.clone(),
        })
    }
}

fn find_terminator(buf: &[u8], from: usize) -> Option<usize> {
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

/// HTTP response head: (status, reason, headers).
pub type ResponseHead = (u16, String, Vec<(String, String)>);

fn parse_head(raw: &[u8]) -> Result<ResponseHead> {
    let text =
        std::str::from_utf8(raw).map_err(|_| Error::Http("non-utf8 response head".into()))?;
    let mut lines = text.split("\r\n").flat_map(|l| l.split('\n'));
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/") {
        return Err(Error::Http(format!(
            "bad status line: {}",
            status_line.trim()
        )));
    }
    let status: u16 = parts
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| Error::Http(format!("bad status code in '{}'", status_line.trim())))?;
    let reason = parts.next().unwrap_or("").trim().to_string();
    let mut headers = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok((status, reason, headers))
}

/// Case-insensitive header lookup.
pub fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// A response with a streaming body. After the body is fully read,
/// `into_conn()` recycles the keep-alive connection.
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    body: BodyState,
    keep_alive: bool,
    tr: Option<Transport>,
    absolute_form: bool,
    proxy_auth: Option<String>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        header_value(&self.headers, name)
    }

    /// Read the entire body into memory (bounded by `max`).
    pub fn read_body(&mut self, max: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = {
                let mut br = BodyReader { resp: self };
                Read::read(&mut br, &mut buf)?
            };
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            if out.len() > max {
                return Err(Error::Http("response body exceeds limit".into()));
            }
        }
        Ok(out)
    }

    /// Borrow the body as a reader.
    pub fn body_reader(&mut self) -> BodyReader<'_> {
        BodyReader { resp: self }
    }

    /// Recycle the connection if the body drained cleanly on a keep-alive.
    pub fn into_conn(self) -> Option<Conn> {
        if self.keep_alive && self.body.finished && !self.body.protocol_error {
            self.tr.map(|tr| Conn {
                tr: Some(tr),
                absolute_form: self.absolute_form,
                proxy_auth: self.proxy_auth,
                poisoned: false,
            })
        } else {
            None
        }
    }

    pub fn content_length(&self) -> Option<u64> {
        self.header("content-length").and_then(|v| v.parse().ok())
    }
}

struct BodyState {
    prelude: Vec<u8>,
    prelude_pos: usize,
    mode: BodyMode,
    finished: bool,
    protocol_error: bool,
}

enum BodyMode {
    Unknown,
    None,
    Sized {
        remaining: u64,
    },
    Chunked {
        remaining_in_chunk: u64,
        done: bool,
        after_crlf: bool,
    },
    UntilEof,
}

impl BodyState {
    fn new(prelude: Vec<u8>) -> BodyState {
        BodyState {
            prelude,
            prelude_pos: 0,
            mode: BodyMode::Unknown,
            finished: false,
            protocol_error: false,
        }
    }

    fn init_mode(&mut self, headers: &[(String, String)], method_head: bool) {
        let chunked = header_value(headers, "transfer-encoding")
            .map(|v| v.to_ascii_lowercase().contains("chunked"))
            .unwrap_or(false);
        if method_head {
            self.mode = BodyMode::None;
            self.finished = true;
        } else if chunked {
            self.mode = BodyMode::Chunked {
                remaining_in_chunk: 0,
                done: false,
                after_crlf: false,
            };
        } else if let Some(len) =
            header_value(headers, "content-length").and_then(|v| v.parse::<u64>().ok())
        {
            self.mode = BodyMode::Sized { remaining: len };
            if len == 0 {
                self.finished = true;
            }
        } else {
            self.mode = BodyMode::UntilEof;
        }
    }

    /// Pull up to `buf.len()` raw body wire bytes, serving any head-read
    /// leftover (prelude) first. All framing modes account prelude bytes.
    fn pull(&mut self, tr: &mut Transport, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.prelude_pos < self.prelude.len() {
            let n = (self.prelude.len() - self.prelude_pos).min(buf.len());
            buf[..n].copy_from_slice(&self.prelude[self.prelude_pos..self.prelude_pos + n]);
            self.prelude_pos += n;
            return Ok(n);
        }
        loop {
            match tr.read(buf) {
                Ok(n) => return Ok(n),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn read_exact_bytes(&mut self, tr: &mut Transport, buf: &mut [u8]) -> std::io::Result<()> {
        let mut filled = 0;
        while filled < buf.len() {
            let n = self.pull(tr, &mut buf[filled..])?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "eof while reading framed body",
                ));
            }
            filled += n;
        }
        Ok(())
    }

    fn read_sized(
        &mut self,
        tr: &mut Transport,
        buf: &mut [u8],
        remaining: &mut u64,
    ) -> std::io::Result<usize> {
        if *remaining == 0 {
            self.finished = true;
            return Ok(0);
        }
        let want = (*remaining as usize).min(buf.len());
        let n = self.pull(tr, &mut buf[..want])?;
        if n == 0 {
            self.protocol_error = true;
            self.finished = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "body shorter than content-length",
            ));
        }
        *remaining -= n as u64;
        if *remaining == 0 {
            self.finished = true;
        }
        Ok(n)
    }

    fn read_chunked(
        &mut self,
        tr: &mut Transport,
        buf: &mut [u8],
        remaining_in_chunk: &mut u64,
        done: &mut bool,
        after_crlf: &mut bool,
    ) -> std::io::Result<usize> {
        if *done {
            self.finished = true;
            return Ok(0);
        }
        if *after_crlf {
            let mut crlf = [0u8; 2];
            self.read_exact_bytes(tr, &mut crlf)?;
            *after_crlf = false;
        }
        if *remaining_in_chunk == 0 {
            // read the chunk-size line byte-wise (bounded)
            let mut line = Vec::new();
            let mut b = [0u8; 1];
            loop {
                self.read_exact_bytes(tr, &mut b)?;
                if b[0] == b'\n' {
                    break;
                }
                if b[0] != b'\r' {
                    line.push(b[0]);
                }
                if line.len() > 1024 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "chunk size line too long",
                    ));
                }
            }
            let size_str = String::from_utf8_lossy(&line);
            let size_str = size_str.split(';').next().unwrap_or("").trim();
            let size = u64::from_str_radix(size_str, 16).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("bad chunk size '{}'", size_str),
                )
            })?;
            if size == 0 {
                // trailers until blank line (bounded)
                let mut t: Vec<u8> = Vec::new();
                let mut b = [0u8; 1];
                loop {
                    self.read_exact_bytes(tr, &mut b)?;
                    t.push(b[0]);
                    if t.ends_with(b"\r\n\r\n") || t == b"\r\n" || t.ends_with(b"\n\n") {
                        break;
                    }
                    if t.len() > 8192 {
                        break;
                    }
                }
                *done = true;
                self.finished = true;
                return Ok(0);
            }
            *remaining_in_chunk = size;
        }
        let want = (*remaining_in_chunk as usize).min(buf.len());
        let n = self.pull(tr, &mut buf[..want])?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof inside chunk data",
            ));
        }
        *remaining_in_chunk -= n as u64;
        if *remaining_in_chunk == 0 {
            *after_crlf = true;
        }
        Ok(n)
    }

    fn read_body(&mut self, tr: &mut Option<Transport>, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.finished && self.prelude_pos >= self.prelude.len() {
            return Ok(0);
        }
        let tr = match tr.as_mut() {
            Some(t) => t,
            None => return Err(std::io::Error::other("body already detached")),
        };
        let mut mode = std::mem::replace(&mut self.mode, BodyMode::Unknown);
        let out = match &mut mode {
            BodyMode::Unknown | BodyMode::None => Ok(0),
            BodyMode::Sized { remaining } => self.read_sized(tr, buf, remaining),
            BodyMode::UntilEof => {
                let n = self.pull(tr, buf)?;
                if n == 0 {
                    self.finished = true;
                }
                Ok(n)
            }
            BodyMode::Chunked {
                remaining_in_chunk,
                done,
                after_crlf,
            } => self.read_chunked(tr, buf, remaining_in_chunk, done, after_crlf),
        };
        self.mode = mode;
        out
    }
}

/// Streaming body reader; borrows the response.
pub struct BodyReader<'a> {
    resp: &'a mut Response,
}

impl Read for BodyReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let r = &mut self.resp;
        r.body.read_body(&mut r.tr, buf)
    }
}

/// High-level one-shot helper (opens a fresh connection; no reuse).
pub fn fetch(req: &Request, cfg: &ClientCfg) -> Result<Response> {
    let mut conn = Conn::connect(&req.url, cfg)?;
    conn.request(req, cfg)
}

/// Follow redirects (up to `max`) and return the final URL plus response.
pub fn resolve_redirects(
    url: &Url,
    cfg: &ClientCfg,
    max: usize,
    method: &str,
) -> Result<(Url, Response)> {
    let mut current = url.clone();
    for _ in 0..=max {
        let req = if method.eq_ignore_ascii_case("HEAD") {
            Request::head(&current)
        } else {
            Request::get(&current)
        };
        let mut conn = Conn::connect(&current, cfg)?;
        let mut resp = conn.request(&req, cfg)?;
        if (301..=308).contains(&resp.status) && resp.status != 306 {
            let loc = resp
                .header("location")
                .ok_or_else(|| Error::Http("redirect without location".into()))?
                .to_string();
            let _ = resp.read_body(1024 * 1024);
            current = current.join(&loc)?;
            continue;
        }
        return Ok((current, resp));
    }
    Err(Error::Http("too many redirects".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parse_cases() {
        let u = Url::parse("https://example.com/a/b?x=1").unwrap();
        assert_eq!(
            (
                u.scheme.as_str(),
                u.host.as_str(),
                u.port,
                u.path_and_query.as_str()
            ),
            ("https", "example.com", 443, "/a/b?x=1")
        );
        let u = Url::parse("http://host:8080").unwrap();
        assert_eq!((u.port, u.path_and_query.as_str()), (8080, "/"));
        assert_eq!(u.host_header(), "host:8080");
        let u = Url::parse("HTTP://EXample.com/x").unwrap();
        assert_eq!(u.scheme, "http");
        assert!(Url::parse("ftp://x/").is_err());
        assert!(Url::parse("nope").is_err());
    }

    #[test]
    fn url_join_cases() {
        let base = Url::parse("https://h.test/dir/page.html?a=1").unwrap();
        assert_eq!(base.join("/root.js").unwrap().path_and_query, "/root.js");
        assert_eq!(
            base.join("next.html").unwrap().path_and_query,
            "/dir/next.html"
        );
        assert_eq!(base.join("https://other/x").unwrap().host, "other");
    }

    #[test]
    fn url_join_normalizes_dot_segments() {
        // HLS playlists reference keys as "../key.bin" relative to a
        // subdirectory playlist; RFC 3986 5.2.4 must remove the dot segments
        let base = Url::parse("https://h.test/live/hi/index.m3u8").unwrap();
        assert_eq!(
            base.join("../key.bin").unwrap().path_and_query,
            "/live/key.bin"
        );
        assert_eq!(
            base.join("seg0.ts").unwrap().path_and_query,
            "/live/hi/seg0.ts"
        );
        // clamping at the root
        let root = Url::parse("https://h.test/a.m3u8").unwrap();
        assert_eq!(root.join("../key.bin").unwrap().path_and_query, "/key.bin");
        assert_eq!(root.join("../../x").unwrap().path_and_query, "/x");
        // current-directory segment and trailing dots
        assert_eq!(base.join("./k").unwrap().path_and_query, "/live/hi/k");
        assert_eq!(base.join(".").unwrap().path_and_query, "/live/hi/");
        assert_eq!(base.join("..").unwrap().path_and_query, "/live/");
        // query strings survive normalization
        assert_eq!(
            base.join("../key.bin?sid=1,2").unwrap().path_and_query,
            "/live/key.bin?sid=1,2"
        );
        // absolute paths are normalized too
        assert_eq!(base.join("/a/../b").unwrap().path_and_query, "/b");
    }

    #[test]
    fn head_parsing() {
        let raw = b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-99/500\r\nContent-Length: 100\r\n\r\n";
        let (status, reason, headers) = parse_head(raw).unwrap();
        assert_eq!(status, 206);
        assert_eq!(reason, "Partial Content");
        assert_eq!(
            header_value(&headers, "content-range"),
            Some("bytes 0-99/500")
        );
        assert_eq!(header_value(&headers, "CONTENT-LENGTH"), Some("100"));
    }

    #[test]
    fn terminator_finding() {
        assert_eq!(find_terminator(b"abc\r\n\r\nbody", 0), Some(3));
        assert_eq!(find_terminator(b"abcdef", 0), None);
        assert_eq!(find_terminator(b"\r\n\r\n", 0), Some(0));
    }
}
