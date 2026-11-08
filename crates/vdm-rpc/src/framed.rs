// SPDX-License-Identifier: GPL-3.0-or-later
//! Length-prefixed JSON frame codec shared by the TCP RPC transport.

use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::MAX_FRAME_BYTES;

/// Writes one frame: u32 LE length + JSON payload.
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: &serde_json::Value) -> io::Result<()> {
    let payload = serde_json::to_vec(value).map_err(io::Error::from)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame exceeds the 64 MiB limit"));
    }
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    writer.write_all(&buf).await?;
    writer.flush().await
}

/// Reads one frame. Returns Ok(None) on clean EOF before any byte of a new
/// frame, and an error on truncated frames or oversized lengths.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<serde_json::Value>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame exceeds the 64 MiB limit"));
    }
    if len == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "zero-length frame"));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload).map(Some).map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn frame_roundtrip() {
        let (mut a, mut b) = duplex(4096);
        write_frame(&mut a, &serde_json::json!({"method": "ping", "id": 1})).await.unwrap();
        let value = read_frame(&mut b).await.unwrap().unwrap();
        assert_eq!(value["method"], "ping");
        assert_eq!(value["id"], 1);
    }

    #[tokio::test]
    async fn partial_writes_are_handled() {
        // A small duplexer forces write_frame to split across flushes and
        // read_exact on the receiving side to buffer partial frames.
        let (mut a, mut b) = duplex(64);
        let payload = serde_json::json!({"hello": true, "pad": "x".repeat(400)});
        let writer = tokio::spawn(async move {
            write_frame(&mut a, &payload).await.unwrap();
        });
        let value = read_frame(&mut b).await.unwrap().unwrap();
        assert_eq!(value["hello"], true);
        assert_eq!(value["pad"].as_str().unwrap().len(), 400);
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn eof_returns_none() {
        let (mut a, mut b) = duplex(64);
        drop(a);
        assert!(read_frame(&mut b).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn oversize_frame_is_rejected() {
        let (mut a, mut b) = duplex(64);
        a.write_all(&u32::MAX.to_le_bytes()).await.unwrap();
        drop(a);
        let err = read_frame(&mut b).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
