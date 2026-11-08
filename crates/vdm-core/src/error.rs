use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// HTTP protocol / transport failure with context.
    Http(String),
    /// Bencode structure failure.
    Bencode(String),
    /// Torrent / magnet failure.
    Torrent(String),
    /// HLS playlist failure.
    Hls(String),
    /// Task-level failure (bad options, failed download, checksum mismatch...).
    Task(String),
    /// JSON (de)serialization failure.
    Json(serde_json::Error),
    #[cfg(feature = "tls")]
    Tls(rustls::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {}", e),
            Error::Http(s) => write!(f, "http: {}", s),
            Error::Bencode(s) => write!(f, "bencode: {}", s),
            Error::Torrent(s) => write!(f, "torrent: {}", s),
            Error::Hls(s) => write!(f, "hls: {}", s),
            Error::Task(s) => write!(f, "task: {}", s),
            Error::Json(e) => write!(f, "json: {}", e),
            #[cfg(feature = "tls")]
            Error::Tls(e) => write!(f, "tls: {}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}
#[cfg(feature = "tls")]
impl From<rustls::Error> for Error {
    fn from(e: rustls::Error) -> Self {
        Error::Tls(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
