// SPDX-License-Identifier: GPL-3.0-or-later
//! Framed JSON-RPC TCP server. Each connection gets an event-forwarding task
//! (daemon notifications) alongside its request loop, so clients never need
//! to poll for progress.

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

use crate::framed::{read_frame, write_frame};
use crate::{make_response, RpcError, RpcHandler};

pub struct ServerHandle {
    pub local_addr: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl ServerHandle {
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

/// Binds `host:port` and serves until shutdown. `events` carries daemon
/// notifications that are forwarded verbatim to every connected client.
pub async fn serve(
    host: &str,
    port: u16,
    handler: Arc<dyn RpcHandler>,
    events: broadcast::Receiver<Value>,
) -> std::io::Result<ServerHandle> {
    let listener = TcpListener::bind((host, port)).await?;
    let local_addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(accept_loop(listener, handler, events, shutdown_rx));
    Ok(ServerHandle { local_addr, shutdown: shutdown_tx })
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
                let Ok((stream, _addr)) = accepted else { return };
                let handler = handler.clone();
                let events = events.resubscribe();
                tokio::spawn(handle_connection(stream, handler, events));
            }
        }
    }
}

async fn handle_connection(
    stream: TcpStream,
    handler: Arc<dyn RpcHandler>,
    events: broadcast::Receiver<Value>,
) {
    let _ = stream.set_nodelay(true);
    let (mut reader, writer) = stream.into_split();
    // Event forwarder and request replies share the write half.
    let writer = Arc::new(tokio::sync::Mutex::new(writer));

    // Event forwarder: notifications have no "id" field.
    let forward = tokio::spawn({
        let writer = writer.clone();
        async move {
            let mut events = events;
            loop {
                match events.recv().await {
                    Ok(event) => {
                        let mut guard = writer.lock().await;
                        if write_frame(&mut *guard, &event).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    });

    // Request loop.
    loop {
        match read_frame(&mut reader).await {
            Ok(Some(value)) => {
                let response = dispatch(&handler, value);
                let mut guard = writer.lock().await;
                if write_frame(&mut *guard, &response).await.is_err() {
                    break;
                }
            }
            Ok(None) => break, // clean EOF
            Err(_) => break,
        }
    }
    forward.abort();
}

/// Processes one JSON-RPC request object. Notifications (no id) produce no
/// reply; malformed envelopes produce an INVALID_REQUEST error with null id.
pub fn dispatch(handler: &Arc<dyn RpcHandler>, value: Value) -> Value {
    let Some(id) = value.get("id").cloned() else {
        // A notification: evaluate for side effects, never reply.
        if let (Some(method), params) = (method_of(&value), params_of(&value)) {
            let _ = handler.call(&method, params);
        }
        return serde_json::json!({ "id": null, "result": null });
    };
    let Some(method) = method_of(&value) else {
        return make_response(&id, Err(RpcError::new(crate::codes::INVALID_REQUEST, "missing method")));
    };
    let params = params_of(&value);
    let result = handler.call(&method, params);
    make_response(&id, result)
}

fn method_of(value: &Value) -> Option<String> {
    value.get("method").and_then(|m| m.as_str()).map(|s| s.to_string())
}

fn params_of(value: &Value) -> &Value {
    value.get("params").unwrap_or(&Value::Null)
}
