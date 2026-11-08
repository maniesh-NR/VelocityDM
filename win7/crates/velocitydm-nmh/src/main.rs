//! VelocityDM browser native messaging host (Chrome/Firefox/Edge/Opera).
//!
//! Wire format (both directions): 4-byte little-endian length + UTF-8 JSON.
//! Commands from the extension:
//!   { "cmd": "ping" }
//!   { "cmd": "add", "url": "...", "filename": "...", "headers": {...},
//!     "referer": "...", "userAgent": "...", "cookies": "..." }
//!   { "cmd": "addBatch", "pattern": "..." }  or  { "cmd": "addBatch", "urls": [...] }
//!   { "cmd": "addTorrent", "magnet": "magnet:?..." }  or  { "cmd": "addTorrent", "torrentPath": "..." }
//!   { "cmd": "status" }
//!   { "cmd": "open" }        // launch the GUI
//! Responses: { "ok": true, ... } / { "ok": false, "error": "..." }

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use vdm_rpc::RpcClient;

fn main() {
    // stdin/stdout are the native-messaging channel; logs go to stderr only
    loop {
        let msg = match read_frame() {
            Some(m) => m,
            None => break, // browser closed the port
        };
        let reply = handle(msg);
        write_frame(&reply);
    }
}

fn handle(msg: Value) -> Value {
    let cmd = msg.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
    match cmd {
        "ping" => json!({ "ok": true, "pong": true, "version": vdm_core::VERSION }),
        "add" => {
            let url = match msg.get("url").and_then(|u| u.as_str()) {
                Some(u) => u.to_string(),
                None => return json!({ "ok": false, "error": "missing url" }),
            };
            let mut options = json!({});
            if let Some(f) = msg.get("filename").and_then(|f| f.as_str()) {
                options["filename"] = json!(f);
            }
            // cookies / referer / UA from the page become download headers
            let mut headers: Vec<(String, String)> = Vec::new();
            if let Some(c) = msg.get("cookies").and_then(|c| c.as_str()) {
                if !c.is_empty() {
                    headers.push(("Cookie".into(), c.to_string()));
                }
            }
            if let Some(r) = msg.get("referer").and_then(|r| r.as_str()) {
                if !r.is_empty() {
                    headers.push(("Referer".into(), r.to_string()));
                }
            }
            if let Some(ua) = msg.get("userAgent").and_then(|u| u.as_str()) {
                if !ua.is_empty() {
                    headers.push(("User-Agent".into(), ua.to_string()));
                }
            }
            if let Some(h) = msg.get("headers").and_then(|h| h.as_object()) {
                for (k, v) in h {
                    if let Some(vs) = v.as_str() {
                        headers.push((k.clone(), vs.to_string()));
                    }
                }
            }
            if !headers.is_empty() {
                options["headers"] = json!(headers);
            }
            if let Some(q) = msg.get("quality").and_then(|q| q.as_str()) {
                options["quality"] = json!(q);
            }
            match call_daemon("downloads.add", json!({ "url": url, "options": options })) {
                Ok(r) => json!({ "ok": true, "id": r["id"].as_u64().unwrap_or(0) }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }
        "addBatch" => {
            let params = if let Some(p) = msg.get("pattern").and_then(|p| p.as_str()) {
                json!({ "pattern": p })
            } else if let Some(list) = msg.get("urls").and_then(|u| u.as_array()) {
                // expand a raw list by adding each URL
                let mut ids = Vec::new();
                for u in list.iter().filter_map(|x| x.as_str()) {
                    if let Ok(r) = call_daemon("downloads.add", json!({ "url": u })) {
                        ids.push(r["id"].as_u64().unwrap_or(0));
                    }
                }
                return json!({ "ok": true, "ids": ids });
            } else {
                return json!({ "ok": false, "error": "missing pattern or urls" });
            };
            match call_daemon("downloads.addBatch", params) {
                Ok(r) => json!({ "ok": true, "ids": r["ids"] }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }
        "addTorrent" => {
            let params = if let Some(m) = msg.get("magnet").and_then(|m| m.as_str()) {
                json!({ "magnet": m })
            } else if let Some(p) = msg.get("torrentPath").and_then(|p| p.as_str()) {
                json!({ "torrentPath": p })
            } else {
                return json!({ "ok": false, "error": "missing magnet or torrentPath" });
            };
            match call_daemon("downloads.addTorrent", params) {
                Ok(r) => json!({ "ok": true, "id": r["id"].as_u64().unwrap_or(0) }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }
        "status" => match call_daemon("downloads.list", json!({})) {
            Ok(list) => json!({ "ok": true, "tasks": list }),
            Err(e) => json!({ "ok": false, "error": e }),
        },
        "open" => {
            // launch the GUI next to this host binary
            let exe = std::env::current_exe().ok();
            let gui = exe
                .as_ref()
                .and_then(|e| e.parent().map(|p| p.join("velocitydm")))
                .filter(|p| p.exists());
            let target = gui.or_else(|| {
                std::env::var("VELOCITYDM_BIN").ok().map(std::path::PathBuf::from)
            });
            match target {
                Some(path) => match std::process::Command::new(path)
                    .arg("gui")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(_) => json!({ "ok": true }),
                    Err(e) => json!({ "ok": false, "error": format!("launch: {}", e) }),
                },
                None => json!({ "ok": false, "error": "velocitydm gui binary not found" }),
            }
        }
        other => json!({ "ok": false, "error": format!("unknown cmd '{}'", other) }),
    }
}

fn call_daemon(method: &str, params: Value) -> Result<Value, String> {
    let addr = daemon_addr();
    let client = RpcClient::new(&addr);
    if let Ok(v) = client.call(method, params.clone()) {
        return Ok(v);
    }
    // daemon not up: try to start it, then retry
    if try_start_daemon(&addr) {
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(200));
            if let Ok(v) = client.call(method, params.clone()) {
                return Ok(v);
            }
        }
        Err("daemon did not start".into())
    } else {
        Err(format!("daemon unreachable at {}", addr))
    }
}

fn daemon_addr() -> String {
    std::env::var("VELOCITYDM_ADDR").unwrap_or_else(|_| vdm_core::RPC_ADDR.to_string())
}

fn try_start_daemon(addr: &str) -> bool {
    let exe = std::env::current_exe().ok();
    let daemon = exe
        .as_ref()
        .and_then(|e| e.parent().map(|p| p.join("velocitydm")))
        .filter(|p| p.exists());
    let target = daemon.or_else(|| {
        std::env::var("VELOCITYDM_BIN").ok().map(std::path::PathBuf::from)
    });
    match target {
        Some(path) => std::process::Command::new(path)
            .args(["daemon", "--addr", addr])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok(),
        None => false,
    }
}

fn read_frame() -> Option<Value> {
    let mut len_buf = [0u8; 4];
    let mut stdin = std::io::stdin().lock();
    // exact read with retry on interrupt
    let mut filled = 0;
    while filled < 4 {
        match stdin.read(&mut len_buf[filled..]) {
            Ok(0) => return None,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 16 * 1024 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; len];
    let mut got = 0;
    while got < len {
        match stdin.read(&mut buf[got..]) {
            Ok(0) => return None,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
    serde_json::from_slice(&buf).ok()
}

fn write_frame(v: &Value) {
    let body = v.to_string();
    let len = (body.len() as u32).to_le_bytes();
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(&len);
    let _ = out.write_all(body.as_bytes());
    let _ = out.flush();
}

#[allow(dead_code)]
fn unused_tcp() {
    let _ = TcpStream::connect("127.0.0.1:1");
}
