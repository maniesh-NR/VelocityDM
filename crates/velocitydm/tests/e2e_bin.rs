// SPDX-License-Identifier: GPL-3.0-or-later
//! Full-stack integration test that drives the REAL binaries:
//! daemon (RPC + SSE) + CLI + native messaging host, against an in-process
//! test HTTP server. This is the "services work with each other" suite.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Daemon {
    child: Child,
    rpc_port: u16,
    sse_port: u16,
    download_dir: std::path::PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        std::fs::remove_dir_all(&self.download_dir).ok();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn spawn_daemon(tmp: &std::path::Path) -> Daemon {
    let rpc_port = free_port();
    let sse_port = free_port();
    let download_dir = tmp.join("downloads");
    let data_dir = tmp.join("data");
    std::fs::create_dir_all(&download_dir).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_velocitydm"))
        .args([
            "--data-dir",
            data_dir.to_str().unwrap(),
            "daemon",
            "--rpc-port",
            &rpc_port.to_string(),
            "--sse-port",
            &sse_port.to_string(),
            "--download-dir",
            download_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn daemon");
    let mut daemon = Daemon { child, rpc_port, sse_port, download_dir };
    wait_for_ready(&mut daemon);
    daemon
}

fn wait_for_ready(daemon: &mut Daemon) {
    let stderr = daemon.child.stderr.take().unwrap();
    let mut lines = BufReader::new(stderr).lines();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "daemon did not become ready");
        match lines.next() {
            Some(Ok(line)) => {
                if line.contains("ready:") {
                    // Keep the stderr reader alive for the lifetime of the
                    // child so the pipe never fills up.
                    std::mem::forget(lines);
                    return;
                }
            }
            _ => panic!("daemon exited before ready"),
        }
    }
}

/// CLI context: writes the settings file the daemon shares, then invokes the
/// real CLI binary against it.
struct CliCtx {
    data_dir: std::path::PathBuf,
}

impl CliCtx {
    fn new(rpc_port: u16) -> CliCtx {
        let dir = std::env::temp_dir().join(format!("vdm-e2e-cli-{}-{}", std::process::id(), rpc_port));
        std::fs::create_dir_all(&dir).unwrap();
        // resolve_dirs(Some(dir)) => (dir, dir/settings.json)
        std::fs::write(
            dir.join("settings.json"),
            format!("{{\"rpc_port\": {}, \"sse_port\": {}}}", rpc_port, rpc_port + 1),
        )
        .unwrap();
        CliCtx { data_dir: dir }
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_velocitydm"))
            .arg("--data-dir")
            .arg(&self.data_dir)
            .args(args)
            .output()
            .expect("failed to run CLI");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).to_string()
                + &String::from_utf8_lossy(&output.stderr),
        )
    }
}

impl Drop for CliCtx {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.data_dir).ok();
    }
}

fn nmh_bin_path() -> std::path::PathBuf {
    // CARGO_BIN_EXE_ is per-package; the NMH lives next to the velocitydm
    // binary in the target directory.
    let velocitydm = env!("CARGO_BIN_EXE_velocitydm");
    let dir = std::path::Path::new(velocitydm).parent().expect("target dir");
    dir.join("velocitydm-nmh")
}

fn nmh_exchange(rpc_port: u16, messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    use serde_json::json;
    let mut child = Command::new(nmh_bin_path())
        .env("VDM_RPC_PORT", rpc_port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn nmh");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut len_buf = [0u8; 4];
        let mut reader = BufReader::new(stdout);
        loop {
            if reader.read_exact(&mut len_buf).is_err() {
                break;
            }
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut payload = vec![0u8; len];
            if reader.read_exact(&mut payload).is_err() {
                break;
            }
            out.push(serde_json::from_slice::<serde_json::Value>(&payload).unwrap());
            if out.len() >= 8 {
                break;
            }
        }
        out
    });
    let mut frames = Vec::new();
    frames.push(json!({ "type": "hello" }));
    for m in messages {
        frames.push(m.clone());
    }
    for frame in frames {
        let payload = serde_json::to_vec(&frame).unwrap();
        stdin.write_all(&(payload.len() as u32).to_le_bytes()).unwrap();
        stdin.write_all(&payload).unwrap();
        stdin.flush().unwrap();
    }
    drop(stdin);
    let got = reader.join().unwrap();
    let _ = child.kill();
    let _ = child.wait();
    got
}

fn wait_completed(cli_ctx: &CliCtx, id: u64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let (code, out) = cli_ctx.run(&["status", &id.to_string()]);
        assert_eq!(code, 0, "status failed: {}", out);
        if out.contains("completed") {
            return true;
        }
        if out.contains("failed") {
            return false;
        }
        assert!(Instant::now() < deadline, "download did not settle; last: {}", out);
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn read_sse_events(port: u16, seconds: u64) -> usize {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("sse connect");
    stream
        .write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    stream.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut events = 0;
    let mut buf = [0u8; 4096];
    let mut pending = String::new();
    while Instant::now() < deadline {
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => {
                pending.push_str(&String::from_utf8_lossy(&buf[..n]));
                while let Some(pos) = pending.find("\n\n") {
                    let block = pending[..pos].to_string();
                    pending.drain(..pos + 2);
                    if block.starts_with("data: ") {
                        events += 1;
                    }
                }
            }
            Ok(_) => break,
            Err(_) => continue, // timeout tick
        }
    }
    events
}

#[test]
fn full_stack_daemon_cli_nmh_sse() {
    use serde_json::json;
    let tmp = std::env::temp_dir().join(format!("vdm-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    // Test payload served over a minimal static file server with ranges.
    let payload: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let body = payload.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => return,
            };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let req = String::from_utf8_lossy(&buf);
                let range = req
                    .to_ascii_lowercase()
                    .lines()
                    .find(|l| l.starts_with("range:"))
                    .and_then(|l| l.split_once(':'))
                    .and_then(|(_, v)| v.trim().strip_prefix("bytes="))
                    .map(|s| s.to_string());
                let total = body.len();
                let head = match range.as_deref().and_then(parse_range) {
                    Some((s, e)) => format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\n\r\n",
                        s, e, total, e - s + 1
                    ),
                    None => format!(
                        "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\n\r\n",
                        total
                    ),
                };
                stream.write_all(head.as_bytes()).unwrap();
                if !req.starts_with("HEAD") {
                    match range.as_deref().and_then(parse_range) {
                        Some((s, e)) => stream.write_all(&body[s..=e]).unwrap(),
                        None => stream.write_all(&body).unwrap(),
                    }
                }
                stream.flush().unwrap();
            });
        }
    });

    let mut daemon = spawn_daemon(&tmp);
    let cli_ctx = CliCtx::new(daemon.rpc_port);

    // 1. CLI add --wait downloads the file end to end.
    let url = format!("http://{}/payload.bin", addr);
    let (code, out) = cli_ctx.run(&["add", &url, "--wait"]);
    assert_eq!(code, 0, "add --wait failed: {}", out);
    assert!(out.contains("completed:"), "expected completion line, got: {}", out);

    // The file matches the served payload byte for byte.
    let downloaded = std::fs::read(daemon.download_dir.join("payload.bin")).unwrap();
    assert_eq!(downloaded, payload, "downloaded bytes must match the server payload");

    // 2. CLI list reports the completed task; stats reports pool reuse.
    let (code, out) = cli_ctx.run(&["list", "--json"]);
    assert_eq!(code, 0);
    assert!(out.contains("payload.bin") && out.contains("completed"), "list: {}", out);
    let (code, out) = cli_ctx.run(&["stats"]);
    assert_eq!(code, 0, "stats: {}", out);
    assert!(out.contains("\"reuses\""), "stats must report pool reuse: {}", out);

    // 3. NMH handshake and RPC round trip through the real host binary.
    let messages = nmh_exchange(
        daemon.rpc_port,
        &[json!({ "type": "rpc", "id": 1, "method": "ping", "params": {} })],
    );
    let hello = &messages[0];
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["daemon"], "running", "nmh must see the daemon");
    assert_eq!(hello["name"], "velocitydm-nmh");
    let pong = &messages[1];
    assert_eq!(pong["type"], "rpcResult");
    assert_eq!(pong["ok"], true);
    assert_eq!(pong["result"]["name"], "velocitydm");

    // 4. NMH add through the real host: queue a second download.
    let messages = nmh_exchange(
        daemon.rpc_port,
        &[json!({
            "type": "rpc",
            "id": 7,
            "method": "add",
            "params": { "url": format!("http://{}/payload-nmh.bin", addr), "filename": "via-nmh.bin" }
        })],
    );
    let result = &messages[1];
    assert_eq!(result["type"], "rpcResult");
    assert_eq!(result["ok"], true, "nmh add failed: {}", result);
    let nmh_id = result["result"]["id"].as_u64().expect("add returns id");
    assert!(wait_completed(&cli_ctx, nmh_id, Duration::from_secs(60)), "nmh download failed");

    let via_nmh = std::fs::read(daemon.download_dir.join("via-nmh.bin")).unwrap();
    assert_eq!(via_nmh, payload, "nmh-initiated download must be byte-exact");

    // 5. SSE stream delivers progress/state events while a download runs.
    let sse_port = daemon.sse_port;
    let sse_reader = std::thread::spawn(move || read_sse_events(sse_port, 5));
    std::thread::sleep(Duration::from_millis(200));
    let (code, out) = cli_ctx.run(&["add", &format!("http://{}/sse-download.bin", addr), "--out", "sse-dl.bin"]);
    assert_eq!(code, 0, "add for sse: {}", out);
    std::thread::sleep(Duration::from_millis(1500));
    let events = sse_reader.join().unwrap();
    assert!(events >= 1, "expected SSE events during a download, got {}", events);

    // 6. CLI pause/resume cycle on a fresh rate-limited download.
    let (code, out) = cli_ctx.run(&["set", "global_rate_limit_bps", "262144"]);
    assert_eq!(code, 0, "set rate limit: {}", out);
    let (code, out) = cli_ctx.run(&["add", &format!("http://{}/paced.bin", addr), "--out", "paced.bin"]);
    assert_eq!(code, 0, "add paced: {}", out);
    let paced_id: u64 = out
        .lines()
        .find_map(|l| l.strip_prefix("queued download #").map(|s| s.trim().parse::<u64>().unwrap()))
        .expect("queued id");
    std::thread::sleep(Duration::from_millis(600));
    let (code, _) = cli_ctx.run(&["pause", &paced_id.to_string()]);
    assert_eq!(code, 0, "pause failed");
    let (code, out) = cli_ctx.run(&["status", &paced_id.to_string()]);
    assert_eq!(code, 0);
    assert!(out.contains("paused"), "expected paused state: {}", out);
    let (code, _) = cli_ctx.run(&["resume", &paced_id.to_string()]);
    assert_eq!(code, 0, "resume failed");
    assert!(wait_completed(&cli_ctx, paced_id, Duration::from_secs(120)), "paced download failed");
    let paced = std::fs::read(daemon.download_dir.join("paced.bin")).unwrap();
    assert_eq!(paced, payload, "paused/resumed download must be byte-exact");

    // 7. Global rate limit is visible via get.
    let (code, out) = cli_ctx.run(&["get", "global_rate_limit_bps"]);
    assert_eq!(code, 0);
    assert!(out.contains("262144"), "get rate limit: {}", out);

    // 8. Graceful shutdown via RPC; the daemon process exits cleanly.
    let (code, out) = cli_ctx.run(&["shutdown"]);
    assert_eq!(code, 0, "shutdown: {}", out);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match daemon.child.try_wait().expect("wait daemon") {
            Some(status) => {
                assert!(status.success(), "daemon exited with {}", status);
                break;
            }
            None => {
                assert!(Instant::now() < deadline, "daemon did not exit after shutdown");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    std::fs::remove_dir_all(&tmp).ok();
}

fn parse_range(spec: &str) -> Option<(usize, usize)> {
    let mut it = spec.splitn(2, '-');
    let s: usize = it.next()?.trim().parse().ok()?;
    let e: usize = it.next()?.trim().parse().ok()?;
    Some((s, e))
}
