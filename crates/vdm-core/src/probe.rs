// SPDX-License-Identifier: GPL-3.0-or-later
//! Server capability probe: HEAD first, then a 1-byte ranged GET fallback.
//! Produces total size, range support, ETag, and a filename guess.

use crate::error::{Result, VdmError};
use crate::httpc::{HttpClient, Method};
use crate::range_pool::{filename_from_disposition, filename_from_url, sanitize_filename};

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub final_url: String,
    pub total: Option<u64>,
    pub accepts_ranges: bool,
    pub etag: Option<String>,
    pub filename: Option<String>,
    pub content_type: Option<String>,
}

pub async fn probe(client: &HttpClient, url: &str, headers: &[(String, String)]) -> Result<ProbeResult> {
    // HEAD first: cheapest capability query. Some servers mishandle HEAD;
    // the ranged GET below is authoritative when HEAD fails or lies.
    if let Ok(resp) = client.head(url, headers).await {
        if resp.status == 200 {
            let accepts = resp.header("accept-ranges").map(|v| v.to_ascii_lowercase().contains("bytes")).unwrap_or(false);
            let total = resp.content_length();
            if accepts || total.is_some() {
                return Ok(from_parts(resp, url, total, accepts));
            }
        }
    }

    // Fallback: ranged GET of one byte.
    let resp = client.request(Method::Get, url, headers, Some((0, Some(0)))).await?;
    match resp.status {
        206 => {
            let total = resp
                .header("content-range")
                .and_then(parse_content_range_total);
            let accepts = true;
            Ok(from_parts(resp, url, total, accepts))
        }
        200 => {
            // Server ignored the Range header: no range support.
            let total = resp.content_length();
            Ok(from_parts(resp, url, total, false))
        }
        code => Err(VdmError::Status { code, url: url.to_string() }),
    }
}

fn from_parts(resp: crate::httpc::Response, original_url: &str, total: Option<u64>, accepts_ranges: bool) -> ProbeResult {
    let etag = resp.header("etag").map(|s| s.to_string());
    let content_type = resp.header("content-type").map(|s| s.to_string());
    let disposition = resp.header("content-disposition").map(|s| s.to_string());
    let final_url = if resp.final_url.is_empty() { original_url.to_string() } else { resp.final_url.clone() };
    let filename = disposition
        .as_deref()
        .and_then(filename_from_disposition)
        .or_else(|| url::Url::parse(&final_url).ok().and_then(|u| filename_from_url(&u)))
        .and_then(|n| sanitize_filename(&n));
    ProbeResult { final_url, total, accepts_ranges, etag, filename, content_type }
}

/// Parses the total out of `Content-Range: bytes 0-0/12345` (`*` total = unknown).
pub fn parse_content_range_total(value: &str) -> Option<u64> {
    let slash = value.rfind('/')?;
    let total_part = value[slash + 1..].trim();
    if total_part == "*" {
        return None;
    }
    total_part.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_parsing() {
        assert_eq!(parse_content_range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(parse_content_range_total("bytes 5-9/*"), None);
        assert_eq!(parse_content_range_total("bytes 0-0/x"), None);
        assert_eq!(parse_content_range_total("garbage"), None);
    }
}
