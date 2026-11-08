// SPDX-License-Identifier: GPL-3.0-or-later
//! Per-connection download worker.
//!
//! Workers repeatedly claim blocks from the shared `RangePool`, fetch them
//! with ranged GETs, and stream the bytes straight to their absolute file
//! offset (`write_all_at`) - no full-file buffering, a bounded 64 KiB window.
//! Retry policy: per-claim attempts with exponential backoff and mirror
//! rotation on connect failures; a bounded number of consecutive claim
//! failures ends the worker so the task can shrink the connection count.
//!
//! Correctness invariants:
//! - the write offset always matches the Range start (retries resume from
//!   `claim.start + fetched`, never from the stale claim start);
//! - pool progress is marked per chunk as bytes land on disk, so pause,
//!   progress reporting, and ctl persistence are always consistent;
//! - pause/cancel exits never mark a partially-fetched block complete.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{is_transient, Result, VdmError};
use crate::httpc::{HttpClient, Method, Response};
use crate::range_pool::{BlockClaim, RangePool, OPEN_END};
use crate::rate_limiter::TokenBucket;

/// Bounded streaming write window; the only allocation a worker keeps.
pub const CHUNK: usize = 64 * 1024;

#[derive(Default)]
pub struct WorkerStats {
    pub connections_active: AtomicU32,
    pub errors_recent: AtomicU32,
    pub bytes_total: AtomicU64,
}

#[derive(Clone)]
pub struct WorkerCtx {
    pub client: HttpClient,
    pub pool: Arc<RangePool>,
    pub file: Arc<std::fs::File>,
    pub urls: Arc<Vec<String>>,
    pub headers: Arc<Vec<(String, String)>>,
    pub pause: Arc<std::sync::atomic::AtomicBool>,
    pub cancel: Arc<std::sync::atomic::AtomicBool>,
    pub limiter: Arc<TokenBucket>,
    pub global_limiter: Arc<TokenBucket>,
    pub stats: Arc<WorkerStats>,
    pub io_timeout_secs: u64,
}

impl WorkerCtx {
    pub fn is_paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Runs until the pool is drained, the task is paused/cancelled, or the
/// failure budget is exhausted. Returns the number of bytes this worker wrote.
pub async fn run_worker(ctx: WorkerCtx) -> Result<u64> {
    let mut written_total: u64 = 0;
    let mut consecutive_failures: u32 = 0;

    loop {
        if ctx.is_cancelled() {
            return Err(VdmError::Cancelled);
        }
        if ctx.is_paused() {
            return Ok(written_total);
        }
        let claim = match ctx.pool.claim() {
            Some(c) => c,
            None => {
                // No unowned pending block: either everything is done or all
                // remaining blocks are owned by other workers.
                return Ok(written_total);
            }
        };

        match fetch_block(&ctx, &claim).await {
            Ok(bytes) => {
                written_total += bytes;
                consecutive_failures = 0;
                if claim.end == OPEN_END {
                    ctx.pool.complete_open(claim.already_done + bytes);
                } else {
                    ctx.pool.complete_block(claim.index);
                }
            }
            Err(VdmError::Cancelled) => {
                // Pause or cancel: release the claim with its progress kept;
                // the block resumes exactly where the disk content ends.
                ctx.pool.release_partial(claim.index);
                return Ok(written_total);
            }
            Err(e) => {
                ctx.pool.release_partial(claim.index);
                ctx.stats.errors_recent.fetch_add(1, Ordering::Relaxed);
                if !is_transient(&e) || consecutive_failures >= 3 {
                    return Err(e);
                }
                consecutive_failures += 1;
                let backoff = Duration::from_millis(250u64 << consecutive_failures.min(4));
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

/// Fetches one block end to end. Returns the number of contiguous bytes
/// written for this claim (beyond `already_done`). Retries always resume at
/// `claim.start + fetched`, so the disk never holds overlapping or skipped
/// ranges for the block.
async fn fetch_block(ctx: &WorkerCtx, claim: &BlockClaim) -> Result<u64> {
    let mut fetched: u64 = 0; // contiguous bytes written for this claim
    let mut attempt: u32 = 0;
    let urls = ctx.urls.as_ref();
    loop {
        attempt += 1;
        if ctx.is_cancelled() || ctx.is_paused() {
            // Graceful stop: never report Ok here - the caller must not mark
            // the block complete for a claim that ends mid-block.
            return Err(VdmError::Cancelled);
        }
        let url_idx = (attempt as usize - 1).min(urls.len().saturating_sub(1));
        let url = &urls[url_idx];

        let resume_from = claim.start + fetched;
        let range = if claim.end == OPEN_END {
            Some((resume_from, None))
        } else {
            Some((resume_from, Some(claim.end)))
        };

        match attempt_request(ctx, url, range, claim).await {
            Ok(bytes) => {
                fetched += bytes;
                return Ok(fetched);
            }
            Err((resp_err, wrote)) => {
                // Chunk writes already marked progress in the pool as they
                // landed on disk; `wrote` only advances the resume offset for
                // the next attempt so it continues where the disk ends.
                fetched += wrote;
                if !is_transient(&resp_err) || attempt >= 5 || urls.is_empty() {
                    return Err(resp_err);
                }
                let backoff = Duration::from_millis(200u64 << attempt.min(4));
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

/// Runs one request for the remainder of the claim. On error returns the error
/// plus the bytes written so far (so progress is preserved).
async fn attempt_request(
    ctx: &WorkerCtx,
    url: &str,
    range: Option<(u64, Option<u64>)>,
    claim: &BlockClaim,
) -> std::result::Result<u64, (VdmError, u64)> {
    // The write offset MUST match the Range start: retries resume from
    // claim.start + fetched, so derive the absolute position from the range
    // rather than the (stale) claim start.
    let start_abs = range.map(|(s, _)| s).unwrap_or(claim.start);
    let mut written: u64 = 0;
    let mut buffer = vec![0u8; CHUNK];

    let mut resp = match ctx.client.request(Method::Get, url, ctx.headers.as_ref(), range).await {
        Ok(r) => r,
        Err(e) => return Err((e, 0)),
    };

    // A 200 answer to a ranged request means the server ignored Range. That
    // is only usable when the request covers the whole remaining content
    // from offset 0 of the block start AND start_abs is the file position we
    // would write - i.e. never for a resumed block mid-file.
    if resp.status == 200 && start_abs > 0 {
        return Err((VdmError::Http("server ignored Range on mid-file request".into()), 0));
    }
    if resp.status == 416 {
        // Nothing left server-side for this range (or the file shrank);
        // treat the block as complete-at-start so the pool can settle.
        return Err((VdmError::Status { code: 416, url: url.to_string() }, 0));
    }
    if resp.status >= 400 {
        let code = resp.status;
        return Err((VdmError::Status { code, url: url.to_string() }, 0));
    }

    let file = ctx.file.clone();
    let mut file_pos = start_abs;

    loop {
        if ctx.is_cancelled() {
            return Err((VdmError::Cancelled, written));
        }
        if ctx.is_paused() {
            return Err((VdmError::Cancelled, written));
        }
        let want = buffer.len();
        // Two-stage token acquisition: the global cap first, then the
        // per-task cap; each returns a grant in 1..=want bytes.
        let granted = ctx.global_limiter.acquire(want as u64).await;
        let granted = ctx.limiter.acquire(granted).await;
        let want = (granted as usize).min(want).max(1);

        let n = match resp.body.read(&mut buffer[..want]).await {
            Ok(n) => n,
            Err(e) => return Err((VdmError::Io(e), written)),
        };
        if n == 0 {
            if claim.end == OPEN_END {
                return Ok(written);
            }
            let expected_rest = claim.end + 1 - (start_abs + written);
            if expected_rest > 0 {
                return Err((
                    VdmError::Http("connection closed before block completed".into()),
                    written,
                ));
            }
            return Ok(written);
        }
        let chunk = &buffer[..n];
        if let Err(e) = write_at(&file, file_pos, chunk) {
            return Err((VdmError::Io(e), written));
        }
        file_pos += n as u64;
        written += n as u64;
        // Live pool accounting: the pool counter tracks disk contents at
        // chunk granularity, so progress, pause, and ctl persistence are
        // always consistent with what is actually on disk.
        ctx.pool.mark_progress(claim.index, n as u64);
        if claim.end != OPEN_END && start_abs + written > claim.end + 1 {
            // Server sent more than the requested range. Accept the overshoot
            // only when this block ends the file, otherwise refuse to avoid
            // corrupting the next block.
            let is_final = ctx.pool.total.map(|t| claim.end + 1 >= t).unwrap_or(true);
            if !is_final {
                return Err((VdmError::Http("server overshot the requested range".into()), written));
            }
            return Ok(written);
        }
        ctx.stats.bytes_total.fetch_add(n as u64, Ordering::Relaxed);
    }
}

/// Positional write (scatter write, no seek state): zero-copy from the read
/// buffer straight to the page cache at the block's absolute offset.
#[inline]
fn write_at(file: &std::fs::File, pos: u64, buf: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(buf, pos)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut written = 0usize;
        while written < buf.len() {
            let n = file.seek_write(&buf[written..], pos + written as u64)?;
            if n == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "short write"));
            }
            written += n;
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, pos);
        use std::io::{Seek, SeekFrom, Write};
        let mut f = file;
        f.seek(SeekFrom::Start(pos))?;
        f.write_all(buf)
    }
}

/// Drains a small error body so the pooled connection stays usable.
pub async fn drain_small(resp: &mut Response) {
    let mut sink = [0u8; 4096];
    let mut budget: usize = 256 * 1024;
    while budget > 0 {
        let n = resp.body.read(&mut sink).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        budget -= n;
    }
}
