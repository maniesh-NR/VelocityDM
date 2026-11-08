//! Proxy support (XDM/Motrix feature): HTTP CONNECT and SOCKS5.

use crate::error::{Error, Result};
use crate::util::base64_encode;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, TcpStream};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProxyConfig {
    Http {
        host: String,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pass: Option<String>,
    },
    Socks5 {
        host: String,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pass: Option<String>,
    },
}

impl ProxyConfig {
    pub fn host(&self) -> &str {
        match self {
            ProxyConfig::Http { host, .. } | ProxyConfig::Socks5 { host, .. } => host,
        }
    }
    pub fn port(&self) -> u16 {
        match self {
            ProxyConfig::Http { port, .. } | ProxyConfig::Socks5 { port, .. } => *port,
        }
    }
    pub fn creds(&self) -> Option<(&str, &str)> {
        match self {
            ProxyConfig::Http { user, pass, .. } | ProxyConfig::Socks5 { user, pass, .. } => {
                match (user, pass) {
                    (Some(u), Some(p)) => Some((u.as_str(), p.as_str())),
                    _ => None,
                }
            }
        }
    }

    /// `Basic ...` header value for Proxy-Authorization (HTTP proxy only).
    pub fn basic_auth(&self) -> Option<String> {
        if !matches!(self, ProxyConfig::Http { .. }) {
            return None;
        }
        self.creds()
            .map(|(u, p)| format!("Basic {}", base64_encode(format!("{}:{}", u, p).as_bytes())))
    }

    /// Parse `http://[user:pass@]host:port` or `socks5://[user:pass@]host:port`.
    pub fn parse_url(s: &str) -> Result<ProxyConfig> {
        let (scheme, rest) = s
            .split_once("://")
            .ok_or_else(|| Error::Http(format!("proxy URL has no scheme: {}", s)))?;
        let (authority, _) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, authority),
        };
        let (host, port) = hostport
            .rsplit_once(':')
            .map(|(h, p)| (h.to_string(), p.parse::<u16>().ok()))
            .filter(|(_, p)| p.is_some())
            .map(|(h, p)| (h, p.unwrap()))
            .ok_or_else(|| Error::Http("proxy URL needs host:port".into()))?;
        if host.is_empty() {
            return Err(Error::Http("proxy host empty".into()));
        }
        let (user, pass) = match userinfo {
            Some(u) => match u.split_once(':') {
                Some((a, b)) => (Some(a.to_string()), Some(b.to_string())),
                None => (Some(u.to_string()), None),
            },
            None => (None, None),
        };
        match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" => Ok(ProxyConfig::Http {
                host,
                port,
                user,
                pass,
            }),
            "socks5" | "socks5h" => Ok(ProxyConfig::Socks5 {
                host,
                port,
                user,
                pass,
            }),
            other => Err(Error::Http(format!("unsupported proxy scheme '{}'", other))),
        }
    }
}

/// A TCP stream to (or tunneled through) a proxy.
pub struct ProxyStream {
    tcp: TcpStream,
}

impl ProxyStream {
    /// Connect to the proxy itself.
    pub fn connect_tcp(cfg: &ProxyConfig, timeout: Duration) -> Result<ProxyStream> {
        let addr = (cfg.host().to_string(), cfg.port());
        let tcp = TcpStream::connect(&addr)
            .map_err(|e| Error::Http(format!("connect proxy {}: {}", cfg.host(), e)))?;
        tcp.set_read_timeout(Some(timeout))
            .and_then(|_| tcp.set_write_timeout(Some(timeout)))?;
        let _ = tcp.set_nodelay(true);
        Ok(ProxyStream { tcp })
    }

    /// Establish an HTTP CONNECT tunnel to `host:port` (for TLS origins).
    pub fn http_connect(
        cfg: &ProxyConfig,
        mut stream: ProxyStream,
        host: &str,
        port: u16,
        _timeout: Duration,
    ) -> Result<ProxyStream> {
        let mut req = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
        if let Some(auth) = cfg.basic_auth() {
            req.push_str(&format!("Proxy-Authorization: {}\r\n", auth));
        }
        req.push_str("\r\n");
        stream
            .tcp
            .write_all(req.as_bytes())
            .and_then(|_| stream.tcp.flush())
            .map_err(|e| Error::Http(format!("proxy CONNECT write: {}", e)))?;
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match stream.tcp.read(&mut byte) {
                Ok(0) => return Err(Error::Http("proxy closed during CONNECT".into())),
                Ok(_) => {
                    buf.push(byte[0]);
                    if buf.ends_with(b"\r\n\r\n") {
                        break;
                    }
                    if buf.len() > 8192 {
                        return Err(Error::Http("proxy CONNECT response too large".into()));
                    }
                }
                Err(e) => return Err(Error::Http(format!("proxy CONNECT read: {}", e))),
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let status = text
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .ok_or_else(|| Error::Http("malformed proxy CONNECT response".into()))?;
        if status != "200" {
            return Err(Error::Http(format!("proxy CONNECT failed: {}", status)));
        }
        Ok(stream)
    }

    /// Full SOCKS5 handshake (connect to proxy + greet + auth + CONNECT).
    pub fn socks_connect(
        cfg: &ProxyConfig,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<ProxyStream> {
        let mut s = Self::connect_tcp(cfg, timeout)?;
        let creds = cfg.creds();
        // greeting
        let greeting: &[u8] = if creds.is_some() {
            &[0x05, 0x02, 0x00, 0x02]
        } else {
            &[0x05, 0x01, 0x00]
        };
        s.tcp
            .write_all(greeting)
            .map_err(io("socks greeting write"))?;
        let mut resp = [0u8; 2];
        s.tcp
            .read_exact(&mut resp)
            .map_err(io("socks greeting read"))?;
        if resp[0] != 0x05 {
            return Err(Error::Http("not a SOCKS5 proxy".into()));
        }
        match resp[1] {
            0x00 => {}
            0x02 => {
                let (user, pass) =
                    creds.ok_or_else(|| Error::Http("proxy requires auth".into()))?;
                if user.len() > 255 || pass.len() > 255 {
                    return Err(Error::Http("socks credentials too long".into()));
                }
                let mut auth = vec![0x01, user.len() as u8];
                auth.extend_from_slice(user.as_bytes());
                auth.push(pass.len() as u8);
                auth.extend_from_slice(pass.as_bytes());
                s.tcp.write_all(&auth).map_err(io("socks auth write"))?;
                let mut ar = [0u8; 2];
                s.tcp.read_exact(&mut ar).map_err(io("socks auth read"))?;
                if ar[1] != 0x00 {
                    return Err(Error::Http("socks auth rejected".into()));
                }
            }
            other => return Err(Error::Http(format!("socks method 0x{:02x} refused", other))),
        }
        // CONNECT request
        let mut req = vec![0x05, 0x01, 0x00];
        if let Ok(v4) = host.parse::<Ipv4Addr>() {
            req.push(0x01);
            req.extend_from_slice(&v4.octets());
        } else if let Ok(v6) = host.trim_matches(['[', ']']).parse::<Ipv6Addr>() {
            req.push(0x04);
            req.extend_from_slice(&v6.octets());
        } else {
            if host.len() > 255 {
                return Err(Error::Http("socks target host too long".into()));
            }
            req.push(0x03);
            req.push(host.len() as u8);
            req.extend_from_slice(host.as_bytes());
        }
        req.extend_from_slice(&port.to_be_bytes());
        s.tcp.write_all(&req).map_err(io("socks connect write"))?;
        // response: ver rep rsv atyp bnd.addr bnd.port
        let mut head = [0u8; 4];
        s.tcp
            .read_exact(&mut head)
            .map_err(io("socks connect read"))?;
        if head[1] != 0x00 {
            return Err(Error::Http(format!(
                "socks connect failed (rep=0x{:02x})",
                head[1]
            )));
        }
        let skip = match head[3] {
            0x01 => 4,
            0x04 => 16,
            0x03 => {
                let mut l = [0u8; 1];
                s.tcp.read_exact(&mut l).map_err(io("socks addr read"))?;
                l[0] as usize
            }
            other => return Err(Error::Http(format!("socks unknown atyp 0x{:02x}", other))),
        };
        let mut rest = vec![0u8; skip + 2];
        s.tcp.read_exact(&mut rest).map_err(io("socks addr read"))?;
        Ok(s)
    }

    pub fn into_tcp(self) -> TcpStream {
        self.tcp
    }
}

fn io(ctx: &'static str) -> impl Fn(std::io::Error) -> Error {
    move |e| Error::Http(format!("{}: {}", ctx, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proxy_urls() {
        let p = ProxyConfig::parse_url("http://proxy.test:8080").unwrap();
        assert_eq!(p.host(), "proxy.test");
        assert_eq!(p.port(), 8080);
        let p = ProxyConfig::parse_url("socks5://u:p@127.0.0.1:1080").unwrap();
        assert_eq!(p.creds(), Some(("u", "p")));
        assert!(ProxyConfig::parse_url("ftp://x:1").is_err());
        assert!(ProxyConfig::parse_url("noproxy").is_err());
    }

    #[test]
    fn basic_auth_value() {
        let p = ProxyConfig::parse_url("http://a:b@p.test:3128").unwrap();
        assert_eq!(p.basic_auth().unwrap(), "Basic YTpi");
        let p = ProxyConfig::parse_url("socks5://a:b@p.test:1080").unwrap();
        assert_eq!(p.basic_auth(), None);
    }
}
