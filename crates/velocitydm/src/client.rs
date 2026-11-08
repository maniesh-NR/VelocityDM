// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI client: one-shot framed JSON-RPC calls to a running daemon on
//! 127.0.0.1:36990 (or the port found in the daemon's settings file).

use std::time::Duration;

use serde_json::{json, Value};
use tokio::net::TcpStream;

use vdm_core::settings::Settings;
use vdm_core::types::Snapshot;

/// CLI context (portable flag + data dir) captured from the parsed command
/// line; the daemon port is discovered from the effective settings file the
/// daemon writes at startup, with an env override for tests.
static CLI_CONTEXT: std::sync::OnceLock<(bool, Option<std::path::PathBuf>)> = std::sync::OnceLock::new();

pub fn set_context(portable: bool, data_dir: Option<std::path::PathBuf>) {
    let _ = CLI_CONTEXT.set((portable, data_dir));
}

fn resolve_port() -> u16 {
    if let Ok(port) = std::env::var("VDM_RPC_PORT") {
        if let Ok(port) = port.trim().parse::<u16>() {
            if port > 0 {
                return port;
            }
        }
    }
    let (portable, data_dir) = CLI_CONTEXT.get().cloned().unwrap_or((false, None));
    let (_, config_path) = Settings::resolve_dirs(portable, data_dir);
    Settings::load(&config_path).map(|s| s.rpc_port).unwrap_or(vdm_core::DEFAULT_RPC_PORT)
}

async fn connect_daemon() -> Result<TcpStream, String> {
    let port = resolve_port();
    let port = if port == 0 { vdm_core::DEFAULT_RPC_PORT } else { port };
    TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|_| "VelocityDM daemon is not running (start it with: velocitydm daemon)".to_string())
}

/// Sends one request and reads frames until the matching response arrives;
/// event frames are skipped (the CLI does not render live progress).
async fn rpc_call(method: &str, params: Value) -> Result<Value, String> {
    let mut stream = connect_daemon().await?;
    stream.set_nodelay(true).ok();
    let request = json!({ "id": 1u64, "method": method, "params": params });
    vdm_rpc::framed::write_frame(&mut stream, &request)
        .await
        .map_err(|e| format!("write failed: {}", e))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err("daemon did not respond in time".into());
        }
        let frame = tokio::time::timeout(Duration::from_secs(60), vdm_rpc::framed::read_frame(&mut stream))
            .await
            .map_err(|_| "timeout".to_string())?
            .map_err(|e| format!("read failed: {}", e))?;
        match frame {
            Some(value) => {
                if value.get("id").is_some() {
                    if let Some(err) = value.get("error") {
                        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("daemon error");
                        return Err(msg.to_string());
                    }
                    return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                }
                // else: an event frame; skip.
            }
            None => return Err("daemon closed the connection".into()),
        }
    }
}

/// add --wait: subscribes and prints progress until the task settles.
async fn rpc_call_and_wait(id: u64) -> Result<Value, String> {
    let mut stream = connect_daemon().await?;
    stream.set_nodelay(true).ok();
    vdm_rpc::framed::write_frame(&mut stream, &json!({ "id": 1, "method": "status", "params": { "id": id } }))
        .await
        .map_err(|e| e.to_string())?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3600);
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err("timeout waiting for completion".into());
        }
        let frame = vdm_rpc::framed::read_frame(&mut stream)
            .await
            .map_err(|e| format!("read failed: {}", e))?;
        match frame {
            Some(value) => {
                if value.get("id").is_some() {
                    continue; // initial status response
                }
                let event = value.get("event").and_then(|e| e.as_str()).unwrap_or("");
                let params = value.get("params").cloned().unwrap_or(Value::Null);
                match event {
                    "progress" => {
                        let snap: Result<Snapshot, _> = serde_json::from_value(params.clone());
                        if let Ok(snap) = snap {
                            if snap.id == id {
                                print_progress(&snap);
                            }
                        }
                    }
                    "completed" => {
                        if params.get("id").and_then(|v| v.as_u64()) == Some(id) {
                            eprintln!();
                            return Ok(params);
                        }
                    }
                    "failed" => {
                        if params.get("id").and_then(|v| v.as_u64()) == Some(id) {
                            eprintln!();
                            return Err(format!(
                                "download failed: {}",
                                params.get("error").and_then(|e| e.as_str()).unwrap_or("unknown")
                            ));
                        }
                    }
                    _ => {}
                }
            }
            None => return Err("daemon closed the connection".into()),
        }
    }
}

fn print_progress(snap: &Snapshot) {
    let total = snap.total.unwrap_or(0);
    let pct = if total > 0 { (snap.downloaded as f64 / total as f64 * 100.0).min(100.0) } else { 0.0 };
    let bar = if total > 0 {
        let filled = (pct / 4.0).round() as usize;
        let filled = filled.min(25);
        format!("{}{}", "#".repeat(filled), "-".repeat(25 - filled))
    } else {
        "-".repeat(25)
    };
    let speed = format_speed(snap.speed_bps);
    eprint!("\r[{}] {:5.1}%  {}  ", bar, pct, speed);
}

fn format_speed(bps: f64) -> String {
    const UNITS: [&str; 5] = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"];
    let mut value = bps;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", value, UNITS[unit])
}

pub async fn cmd_add(spec: vdm_core::TaskSpec, wait: bool) -> i32 {
    let params = match serde_json::to_value(&spec) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}", e);
            return 2;
        }
    };
    match rpc_call("add", params).await {
        Ok(result) => {
            let id = result.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("queued download #{}", id);
            if wait {
                match rpc_call_and_wait(id).await {
                    Ok(done) => {
                        let path = done.get("path").and_then(|p| p.as_str()).unwrap_or("");
                        let bytes = done.get("bytes").and_then(|b| b.as_u64()).unwrap_or(0);
                        println!("completed: {} ({} bytes)", path, bytes);
                        0
                    }
                    Err(e) => {
                        eprintln!("error: {}", e);
                        1
                    }
                }
            } else {
                0
            }
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

fn print_snapshots(snaps: &[Snapshot]) {
    println!(
        "{:>4}  {:<9}  {:>12}  {:>12}  {:>10}  {}",
        "ID", "STATE", "DOWNLOADED", "TOTAL", "SPEED", "FILENAME"
    );
    for s in snaps {
        let total = s.total.map(|t| t.to_string()).unwrap_or_else(|| "?".into());
        println!(
            "{:>4}  {:<9}  {:>12}  {:>12}  {:>10}  {}",
            s.id,
            s.state.as_str(),
            s.downloaded,
            total,
            format_speed(s.speed_bps),
            s.filename
        );
    }
}

pub async fn cmd_list(json: bool) -> i32 {
    match rpc_call("list", json!({})).await {
        Ok(result) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
            } else {
                let snaps: Vec<Snapshot> = serde_json::from_value(result).unwrap_or_default();
                if snaps.is_empty() {
                    println!("no downloads");
                } else {
                    print_snapshots(&snaps);
                }
            }
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

pub async fn cmd_status(id: u64) -> i32 {
    match rpc_call("status", json!({ "id": id })).await {
        Ok(result) => {
            let snap: Snapshot = serde_json::from_value(result).unwrap_or_else(|_| Snapshot {
                id,
                url: String::new(),
                filename: String::new(),
                state: vdm_core::TaskState::Removed,
                total: None,
                downloaded: 0,
                speed_bps: 0.0,
                connections: 0,
                eta_secs: None,
                error: None,
                path: None,
            });
            print_snapshots(&[snap]);
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

pub async fn cmd_pause_resume(target: &str, pause: bool) -> i32 {
    let method = if pause { "pause" } else { "resume" };
    let ids: Vec<u64> = if target == "all" {
        match rpc_call("list", json!({})).await {
            Ok(result) => serde_json::from_value::<Vec<Snapshot>>(result)
                .unwrap_or_default()
                .iter()
                .map(|s| s.id)
                .collect(),
            Err(e) => {
                eprintln!("error: {}", e);
                return 1;
            }
        }
    } else {
        match target.parse::<u64>() {
            Ok(id) => vec![id],
            Err(_) => {
                eprintln!("error: target must be a task id or 'all'");
                return 2;
            }
        }
    };
    let mut failures = 0;
    for id in ids {
        match rpc_call(method, json!({ "id": id })).await {
            Ok(result) => {
                let state = result.get("state").and_then(|s| s.as_str()).unwrap_or("?");
                println!("#{} -> {}", id, state);
            }
            Err(e) => {
                eprintln!("error: #{}: {}", id, e);
                failures += 1;
            }
        }
    }
    if failures > 0 {
        1
    } else {
        0
    }
}

pub async fn cmd_remove(id: u64, purge: bool) -> i32 {
    match rpc_call("remove", json!({ "id": id, "purge": purge })).await {
        Ok(_) => {
            println!("removed #{}", id);
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

pub async fn cmd_verify(id: u64) -> i32 {
    match rpc_call("verify", json!({ "id": id })).await {
        Ok(result) => {
            let ok = result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            println!("#{} checksum {}", id, if ok { "OK" } else { "MISMATCH" });
            if ok {
                0
            } else {
                1
            }
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

pub async fn cmd_stats() -> i32 {
    match rpc_call("stats", json!({})).await {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

pub async fn cmd_get_setting(key: &str) -> i32 {
    match rpc_call("get_settings", json!({})).await {
        Ok(result) => {
            let value = result.get(key).cloned().unwrap_or(Value::Null);
            println!("{}", value);
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

pub async fn cmd_set_setting(key: &str, value: &str) -> i32 {
    let parsed: Value = match key {
        "global_rate_limit_bps" | "refresh_hz" => value.parse::<u64>().map(|v| json!(v)).unwrap_or(Value::Null),
        _ => json!(value),
    };
    if parsed.is_null() {
        eprintln!("error: value must be an integer for {}", key);
        return 2;
    }
    match rpc_call("set_settings", json!({ "key": key, "value": parsed })).await {
        Ok(result) => {
            println!("{} = {}", key, result.get("value").cloned().unwrap_or(Value::Null));
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

pub async fn cmd_shutdown() -> i32 {
    match rpc_call("shutdown", json!({})).await {
        Ok(_) => {
            println!("daemon shutting down");
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}
