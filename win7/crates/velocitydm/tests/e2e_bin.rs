// SPDX-License-Identifier: GPL-3.0-or-later
//! Full-stack integration test that drives the REAL binaries against an
//! in-process range-capable test HTTP server:
//!
//! - `velocitydm daemon`  (engine + JSON-RPC + SSE on one address)
//! - `velocitydm` CLI     (add / list / status / pause / resume / limits)
//! - `velocitydm-nmh`     (browser native messaging host, framed protocol)
//!
//! This is the "services work with each other" suite; the shell-level twin
//! with torrent/magnet/batch coverage lives in scripts/e2e.sh.

use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;
use vdm_rpc::RpcClient;

struct Daemon {
    child: Child,
    addr: String,
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

/// Spawn the real daemon on an ephemeral port with the config redirected
/// into the test sandbox (XDG_CONFIG_HOME is honoured on every platform,
/// see daemon::config_path). Readiness is polled over RPC (`daemon.ping`).
fn spawn_daemon(tmp: &std::path::Path) -> Daemon {
    let port = free_port();
    let addr = format!("127.0.0.1:{}", port);
    let download_dir = tmp.join("downloads");
    let config_home = tmp.join("config");
    std::fs::create_dir_all(&download_dir).unwrap();
    std::fs::create_dir_all(&config_home).unwrap();
    let stderr_path = tmp.join("daemon.stderr");
    let stderr_file = std::fs::File::create(&stderr_path).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_velocitydm"))
        .arg("--addr")
        .arg(&addr)
        .arg("daemon")
        .env("XDG_CONFIG_HOME", &config_home)
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("failed to spawn daemon");
    let mut daemon = Daemon {
        child,
        addr,
        download_dir,
    };

    // Poll RPC readiness the same way the CLI's ensure_daemon does.
    let deadline = Instant::now() + Duration::from_secs(30);
    let client = RpcClient::new(&daemon.addr);
    loop {
        assert!(
            Instant::now() < deadline,
            "daemon did not come up within 30s; stderr: {}",
            std::fs::read_to_string(&stderr_path).unwrap_or_default()
        );
        match daemon.child.try_wait() {
            Ok(Some(status)) => panic!(
                "daemon exited early with {}: {}",
                status,
                std::fs::read_to_string(&stderr_path).unwrap_or_default()
            ),
            Ok(None) => {}
            Err(e) => panic!("wait daemon: {}", e),
        }
        if client.call("daemon.ping", json!({})).is_ok() {
            return daemon;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Run the real CLI against the daemon address; returns (exit code, combined
/// stdout+stderr).
fn cli(addr: &str, args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_velocitydm"))
        .arg("--addr")
        .arg(addr)
        .args(args)
        .output()
        .expect("failed to run CLI");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

/// Invoke the real native messaging host: framed (u32 LE length + JSON)
/// messages on stdin, replies on stdout. Uses VELOCITYDM_ADDR to find the
/// daemon (same contract as the browser extension).
fn nmh_exchange(addr: &str, messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut child = Command::new(nmh_bin_path())
        .env("VELOCITYDM_ADDR", addr)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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
            if len == 0 || len > 16 * 1024 * 1024 {
                break;
            }
            let mut payload = vec![0u8; len];
            if reader.read_exact(&mut payload).is_err() {
                break;
            }
            out.push(serde_json::from_slice::<serde_json::Value>(&payload).unwrap());
            if out.len() >= messages_max() {
                break;
            }
        }
        out
    });
    for frame in messages {
        let payload = serde_json::to_vec(frame).unwrap();
        stdin
            .write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        stdin.write_all(&payload).unwrap();
        stdin.flush().unwrap();
    }
    drop(stdin); // closing stdin ends the host's read loop
    let got = reader.join().unwrap();
    let _ = child.kill();
    let _ = child.wait();
    got
}

fn messages_max() -> usize {
    16
}

fn nmh_bin_path() -> std::path::PathBuf {
    // CARGO_BIN_EXE_ is per-package; the NMH lives next to the velocitydm
    // binary in the target directory.
    let velocitydm = env!("CARGO_BIN_EXE_velocitydm");
    let dir = std::path::Path::new(velocitydm)
        .parent()
        .expect("target dir");
    dir.join("velocitydm-nmh")
}

fn wait_state(addr: &str, id: u64, want: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let (code, out) = cli(addr, &["status", &id.to_string()]);
        assert_eq!(code, 0, "status failed: {}", out);
        if out.contains(&format!("\"state\": \"{}\"", want)) {
            return true;
        }
        // terminal states other than the wanted one mean we will never reach it
        for terminal in ["\"failed\"", "\"completed\"", "\"seeding\""] {
            if want != terminal.trim_matches(|c| c == '"' || c == ' ') && out.contains(terminal) {
                return false;
            }
        }
        assert!(
            Instant::now() < deadline,
            "download did not settle; last: {}",
            out
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Read SSE frames (`data: ...` lines) from `GET /events` on the shared RPC
/// address for `seconds`, returning the number of data frames seen.
fn read_sse_events(addr: &str, seconds: u64) -> usize {
    let mut stream = TcpStream::connect(addr).expect("sse connect");
    stream
        .write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
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

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

fn parse_range(spec: &str) -> Option<(usize, usize)> {
    let mut it = spec.splitn(2, '-');
    let s: usize = it.next()?.trim().parse().ok()?;
    let e: usize = it.next()?.trim().parse().ok()?;
    Some((s, e))
}

#[test]
fn full_stack_daemon_cli_nmh_sse() {
    let tmp = std::env::temp_dir().join(format!(
        "vdm-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    // Test payload served over a minimal static file server with ranges.
    let payload: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
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
    let daddr = daemon.addr.clone();

    // Configure the daemon's default download dir (the same setting the GUI
    // and `settings --dir` expose); extension-initiated downloads land there.
    let dl_dir = daemon.download_dir.to_str().unwrap().to_string();
    let (code, out) = cli(&daddr, &["settings", "--dir", &dl_dir]);
    assert_eq!(code, 0, "settings --dir failed: {}", out);

    // 1. CLI add with checksum: segmented download, byte-exact, no leftovers.
    let sum = sha256_hex(&payload);
    let url = format!("http://{}/payload.bin", addr);
    let (code, out) = cli(
        &daddr,
        &[
            "add",
            &url,
            "--dir",
            &dl_dir,
            "--conn",
            "8",
            "--checksum",
            &format!("sha256:{}", sum),
        ],
    );
    assert_eq!(code, 0, "add failed: {}", out);
    let id: u64 = out
        .lines()
        .find_map(|l| l.strip_prefix("added download #"))
        .and_then(|s| s.trim().parse().ok())
        .expect("added id");
    assert!(
        wait_state(&daddr, id, "completed", Duration::from_secs(60)),
        "download did not complete"
    );
    let downloaded = std::fs::read(daemon.download_dir.join("payload.bin")).unwrap();
    assert_eq!(
        downloaded, payload,
        "downloaded bytes must match the server payload"
    );
    assert!(
        !daemon.download_dir.join("payload.bin.part").exists()
            && !daemon.download_dir.join("payload.bin.vdmctl").exists(),
        "no .part/.vdmctl may survive a completed download"
    );

    // 2. CLI list --json reports the completed task.
    let (code, out) = cli(&daddr, &["list", "--json"]);
    assert_eq!(code, 0, "list failed: {}", out);
    assert!(
        out.contains("payload.bin"),
        "list must mention the file: {}",
        out
    );
    assert!(
        out.contains("completed"),
        "list must report completion: {}",
        out
    );

    // 3. NMH: ping, add (as the browser extension would), status.
    let replies = nmh_exchange(
        &daddr,
        &[
            json!({ "cmd": "ping" }),
            json!({ "cmd": "add", "url": format!("http://{}/from-extension.bin", addr),
                    "filename": "from-extension.bin" }),
            json!({ "cmd": "status" }),
        ],
    );
    assert!(
        replies.len() >= 3,
        "nmh must answer all three frames, got {}",
        replies.len()
    );
    let pong = &replies[0];
    assert_eq!(pong["ok"], true, "nmh ping: {}", pong);
    assert_eq!(pong["pong"], true, "nmh ping must pong: {}", pong);
    let added = &replies[1];
    assert_eq!(added["ok"], true, "nmh add failed: {}", added);
    let nmh_id = added["id"].as_u64().expect("nmh add returns id");
    assert_eq!(replies[2]["ok"], true, "nmh status failed: {}", replies[2]);
    assert!(
        wait_state(&daddr, nmh_id, "completed", Duration::from_secs(60)),
        "extension-initiated download did not complete"
    );
    let via_nmh = std::fs::read(daemon.download_dir.join("from-extension.bin")).unwrap();
    assert_eq!(
        via_nmh, payload,
        "nmh-initiated download must be byte-exact"
    );

    // 4. SSE: the shared address serves /events while a download runs.
    let sse_addr = daddr.clone();
    let sse_reader = std::thread::spawn(move || read_sse_events(&sse_addr, 4));
    std::thread::sleep(Duration::from_millis(200));
    let (code, out) = cli(
        &daddr,
        &[
            "add",
            &format!("http://{}/sse-download.bin", addr),
            "--dir",
            daemon.download_dir.to_str().unwrap(),
            "--out",
            "sse-dl.bin",
        ],
    );
    assert_eq!(code, 0, "add for sse: {}", out);
    let events = sse_reader.join().unwrap();
    assert!(
        events >= 1,
        "expected SSE events during a download, got {}",
        events
    );

    // 5. Rate limit + pause/resume on a paced download.
    let (code, out) = cli(&daddr, &["limits", "--global", "512KB"]);
    assert_eq!(code, 0, "set rate limit: {}", out);
    let (code, out) = cli(&daddr, &["limits"]);
    assert_eq!(code, 0, "show limits: {}", out);
    assert!(
        out.contains("500.0 KiB/s") || out.contains("512"),
        "limit visible: {}",
        out
    );
    let (code, out) = cli(
        &daddr,
        &[
            "add",
            &format!("http://{}/paced.bin", addr),
            "--dir",
            daemon.download_dir.to_str().unwrap(),
            "--out",
            "paced.bin",
        ],
    );
    assert_eq!(code, 0, "add paced: {}", out);
    let paced_id: u64 = out
        .lines()
        .find_map(|l| l.strip_prefix("added download #"))
        .and_then(|s| s.trim().parse().ok())
        .expect("paced id");
    std::thread::sleep(Duration::from_millis(500)); // let it get some bytes in
    let (code, out) = cli(&daddr, &["pause", &paced_id.to_string()]);
    assert_eq!(code, 0, "pause failed: {}", out);
    // The pause signal is asynchronous: poll until the snapshot flips.
    assert!(
        wait_state(&daddr, paced_id, "paused", Duration::from_secs(10)),
        "download did not reach paused state"
    );
    let (code, out) = cli(&daddr, &["resume", &paced_id.to_string()]);
    assert_eq!(code, 0, "resume failed: {}", out);
    assert!(
        wait_state(&daddr, paced_id, "completed", Duration::from_secs(90)),
        "paced download did not finish after resume"
    );
    let paced = std::fs::read(daemon.download_dir.join("paced.bin")).unwrap();
    assert_eq!(paced, payload, "paused/resumed download must be byte-exact");

    // 6. Rate limit can be cleared again.
    let (code, out) = cli(&daddr, &["limits", "--global", "none"]);
    assert_eq!(code, 0, "clear limit: {}", out);
    assert!(out.contains("disabled"), "limit cleared: {}", out);

    // 7. Graceful shutdown via the CLI (which drives the daemon.shutdown
    // RPC); the daemon process exits cleanly.
    let (code, out) = cli(&daddr, &["shutdown"]);
    assert_eq!(code, 0, "shutdown failed: {}", out);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match daemon.child.try_wait().expect("wait daemon") {
            Some(status) => {
                assert!(status.success(), "daemon exited with {}", status);
                break;
            }
            None => {
                assert!(
                    Instant::now() < deadline,
                    "daemon did not exit after shutdown"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    std::fs::remove_dir_all(&tmp).ok();
}
