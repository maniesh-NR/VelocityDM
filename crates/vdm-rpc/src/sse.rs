// SPDX-License-Identifier: GPL-3.0-or-later
//! Minimal HTTP/SSE server for the GUI and browser tooling.
//!
//! Routes:
//! - `GET /events`  -> text/event-stream, one `data:` line per daemon event,
//!   with a 15 s heartbeat comment to survive proxies.
//! - `POST /rpc`    -> one JSON-RPC call in, one JSON-RPC response out.
//! - `GET /health`  -> `ok`.
//!
//! Request heads are parsed with a bounded buffer and POST bodies are read
//! exactly by Content-Length (the head reader never consumes body bytes).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

use crate::dispatch;
use crate::RpcHandler;

const HEAD_LIMIT: usize = 32 * 1024;
const BODY_LIMIT: usize = 1024 * 1024;
const HEARTBEAT: Duration = Duration::from_secs(15);

pub struct SseServerHandle {
    pub local_addr: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl SseServerHandle {
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

pub async fn serve_sse(
    host: &str,
    port: u16,
    handler: Arc<dyn RpcHandler>,
    events: broadcast::Receiver<Value>,
) -> std::io::Result<SseServerHandle> {
    let listener = TcpListener::bind((host, port)).await?;
    let local_addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(accept_loop(listener, handler, events, shutdown_rx));
    Ok(SseServerHandle { local_addr, shutdown: shutdown_tx })
}

async fn accept_loop(
    listener: TcpListener,
    handler: Arc<dyn RpcHandler>,
    events: broadcast::Receiver<Value>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() { return; }
            }
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { return };
                let handler = handler.clone();
                let events = events.resubscribe();
                tokio::spawn(handle_connection(stream, handler, events));
            }
        }
    }
}

struct Request {
    method: String,
    path: String,
    content_length: usize,
}

/// Reads the head (bounded); POST bodies are read by the caller through the
/// same reader, exactly by Content-Length.
async fn read_request(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> std::io::Result<Option<Request>> {
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    let mut tmp = [0u8; 2048];
    let head_end;
    loop {
        if let Some(pos) = find_head_end(&buf) {
            head_end = pos;
            break;
        }
        if buf.len() > HEAD_LIMIT {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "head too large"));
        }
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let path = parts.next().unwrap_or("/").to_string();
    let mut content_length = 0usize;
    for line in lines {
        if let Some(colon) = line.find(':') {
            let name = line[..colon].trim().to_ascii_lowercase();
            let value = line[colon + 1..].trim();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
        }
    }
    if content_length > BODY_LIMIT {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "body too large"));
    }
    Ok(Some(Request { method, path, content_length }))
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

async fn handle_connection(
    stream: TcpStream,
    handler: Arc<dyn RpcHandler>,
    events: broadcast::Receiver<Value>,
) {
    let _ = stream.set_nodelay(true);
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let request = match read_request(&mut reader).await {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(_) => {
            let _ = write_half
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            return;
        }
    };

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/events") => {
            let _ = write_half
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n",
                )
                .await;
            stream_events(&mut write_half, events).await;
        }
        ("POST", "/rpc") => {
            // Read exactly content-length body bytes.
            let mut body = vec![0u8; request.content_length];
            if request.content_length > 0 {
                if reader.read_exact(&mut body).await.is_err() {
                    return;
                }
            }
            let reply = match serde_json::from_slice::<Value>(&body) {
                Ok(value) => dispatch(&handler, value),
                Err(_) => serde_json::json!({
                    "id": null,
                    "error": {"code": crate::codes::PARSE_ERROR, "message": "invalid JSON"}
                }),
            };
            let payload = serde_json::to_vec(&reply).unwrap_or_default();
            let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
            let _ = write_half.write_all(head.as_bytes()).await;
            let _ = write_half.write_all(&payload).await;
            let _ = write_half.shutdown().await;
        }
        ("GET", "/health") => {
            let _ = write_half
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok\n")
                .await;
        }
        _ => {
            let _ = write_half
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        }
    }
}

async fn stream_events(write_half: &mut tokio::net::tcp::OwnedWriteHalf, events: broadcast::Receiver<Value>) {
    let mut events = events;
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        let mut data = Vec::with_capacity(256);
                        data.extend_from_slice(b"data: ");
                        data.extend_from_slice(serde_json::to_string(&event).unwrap_or_default().as_bytes());
                        data.extend_from_slice(b"\n\n");
                        if write_half.write_all(&data).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
            _ = heartbeat.tick() => {
                if write_half.write_all(b": heartbeat\n\n").await.is_err() {
                    return;
                }
            }
        }
    }
}
