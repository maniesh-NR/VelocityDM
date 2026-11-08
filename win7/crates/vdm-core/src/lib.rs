//! VelocityDM engine core.
//!
//! Clean-room implementation. Engine modules:
//! - [`engine`]: HTTP/HTTPS segmented download engine with work-stealing range pool,
//!   mirror failover, AIMD connection tuning and token-bucket rate limiting.
//! - [`hls`]: HLS (m3u8) playlist parsing and segmented stream assembly with AES-128.
//! - [`bt`]: BitTorrent engine (bencode, metainfo, magnet, HTTP tracker, peer wire
//!   protocol, BEP-9 metadata extension, rarest-first piece picking, seeding).
//! - Scheduler, batch URL expansion, queue priorities, proxies, checksums.

pub mod batch;
pub mod bt;
pub mod checksum;
pub mod engine;
pub mod error;
pub mod http;
pub mod proxy;
pub mod range_pool;
pub mod token_bucket;
pub mod util;

#[cfg(feature = "tls")]
pub mod tls;

pub use error::{Error, Result};
pub use token_bucket::TokenBucket;

/// Shared semantic version reported over RPC.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default RPC bind address (loopback only).
pub const RPC_ADDR: &str = "127.0.0.1:36990";
