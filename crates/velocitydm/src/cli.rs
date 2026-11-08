//! CLI command implementations (talk to the daemon over JSON-RPC).

use crate::Commands;
use serde_json::{json, Value};
use vdm_core::util::{format_bytes, now_ms, parse_bytes};
use vdm_rpc::RpcClient;

pub fn client(addr: &str) -> RpcClient {
    RpcClient::new(addr)
}

/// Ensure a daemon answers at `addr`; try to start one if not.
pub fn ensure_daemon(addr: &str) -> Result<(), String> {
    let c = RpcClient::new(addr);
    if c.call("daemon.ping", json!({})).is_ok() {
        return Ok(());
    }
    // spawn ourselves as daemon (detached)
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["daemon", "--addr", addr]);
    let _ = cmd.stdin(std::process::Stdio::null());
    let _ = cmd.stdout(std::process::Stdio::null());
    let _ = cmd.stderr(std::process::Stdio::null());
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to start daemon: {}", e));
    std::mem::forget(child); // detach
                             // wait for readiness
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if c.call("daemon.ping", json!({})).is_ok() {
            return Ok(());
        }
    }
    Err("daemon did not come up within 10s".to_string())
}

fn out_file_dir(opts: &mut Value, out: Option<&str>, dir: Option<&str>) {
    if let Some(o) = out {
        opts["filename"] = json!(o);
    }
    if let Some(d) = dir {
        opts["outDir"] = json!(d);
    }
}

fn apply_common(opts: &mut Value, conn: Option<usize>, limit: Option<&str>) -> Result<(), String> {
    if let Some(c) = conn {
        opts["connections"] = json!(c);
    }
    if let Some(l) = limit {
        let bps = parse_bytes(l).ok_or_else(|| format!("bad limit '{}'", l))?;
        opts["rateLimitBps"] = json!(bps);
    }
    Ok(())
}

fn parse_checksum(s: &str) -> Result<(String, String), String> {
    let (algo, hexv) = s
        .split_once(':')
        .ok_or_else(|| "checksum must be ALGO:HEX (e.g. sha256:ab..)".to_string())?;
    vdm_core::checksum::Algo::parse(algo).map_err(|e| e.to_string())?;
    Ok((algo.to_string(), hexv.to_string()))
}

impl Commands {
    pub fn run_remote(&self, addr: &str) -> i32 {
        match self.execute(addr) {
            Ok(msg) => {
                if !msg.is_empty() {
                    println!("{}", msg);
                }
                0
            }
            Err(e) => {
                eprintln!("error: {}", e);
                if e.contains("connect daemon") {
                    eprintln!("hint: start one with `velocitydm daemon` (it also auto-starts when adding)");
                }
                1
            }
        }
    }

    fn execute(&self, addr: &str) -> Result<String, String> {
        match self {
            Commands::Add {
                url,
                out,
                dir,
                conn,
                mirrors,
                limit,
                checksum,
                quality,
                priority,
                start_at,
                fresh,
            } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                let mut opts = json!({});
                out_file_dir(&mut opts, out.as_deref(), dir.as_deref());
                apply_common(&mut opts, *conn, limit.as_deref())?;
                if !mirrors.is_empty() {
                    opts["mirrors"] = json!(mirrors);
                }
                if let Some(cs) = checksum {
                    let (algo, hexv) = parse_checksum(cs)?;
                    opts["checksumAlgo"] = json!(algo);
                    opts["checksumHex"] = json!(hexv);
                }
                if let Some(q) = quality {
                    opts["quality"] = json!(q);
                }
                if let Some(p) = priority {
                    opts["priority"] = json!(p);
                }
                if let Some(at) = start_at {
                    opts["startAt"] = json!(at);
                }
                if *fresh {
                    opts["fresh"] = json!(true);
                }
                let r = c
                    .call("downloads.add", json!({ "url": url, "options": opts }))
                    .map_err(|e| e.to_string())?;
                Ok(format!("added download #{}", r["id"].as_u64().unwrap_or(0)))
            }
            Commands::Batch { pattern, dir, conn, limit } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                let mut opts = json!({});
                if let Some(d) = dir {
                    opts["outDir"] = json!(d);
                }
                apply_common(&mut opts, *conn, limit.as_deref())?;
                let r = c
                    .call("downloads.addBatch", json!({ "pattern": pattern, "options": opts }))
                    .map_err(|e| e.to_string())?;
                let ids = r["ids"].as_array().map(|a| a.len()).unwrap_or(0);
                Ok(format!("added {} downloads from batch", ids))
            }
            Commands::List { json: as_json } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                let list = c.call("downloads.list", json!({})).map_err(|e| e.to_string())?;
                if *as_json {
                    Ok(serde_json::to_string_pretty(&list).unwrap())
                } else {
                    Ok(render_list(&list))
                }
            }
            Commands::Status { id } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                let s = c
                    .call("downloads.get", json!({ "id": id }))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::to_string_pretty(&s).unwrap())
            }
            Commands::Pause { id } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                match id {
                    Some(i) => {
                        c.call("downloads.pause", json!({ "id": i })).map_err(|e| e.to_string())?;
                        Ok(format!("paused #{}", i))
                    }
                    None => {
                        c.call("downloads.pauseAll", json!({})).map_err(|e| e.to_string())?;
                        Ok("paused all".to_string())
                    }
                }
            }
            Commands::Resume { id } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                c.call("downloads.resume", json!({ "id": id })).map_err(|e| e.to_string())?;
                Ok(format!("resumed #{}", id))
            }
            Commands::Remove { id, delete } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                c.call("downloads.remove", json!({ "id": id, "deleteFiles": delete }))
                    .map_err(|e| e.to_string())?;
                Ok(format!("removed #{}", id))
            }
            Commands::Refresh { id, url } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                c.call("downloads.refresh", json!({ "id": id, "url": url }))
                    .map_err(|e| e.to_string())?;
                Ok(format!("refreshed #{} with new link", id))
            }
            Commands::Torrent { file, dir, files, limit } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                // show file list first when selection is requested
                let info = c
                    .call("torrents.files", json!({ "torrentPath": file }))
                    .map_err(|e| e.to_string())?;
                let nfiles = info["files"].as_array().map(|a| a.len()).unwrap_or(0);
                let mut priorities = vec![1u8; nfiles];
                if let Some(sel) = files {
                    for (i, p) in priorities.iter_mut().enumerate() {
                        *p = if sel.contains(&i) { 1 } else { 0 };
                    }
                    if sel.is_empty() {
                        priorities = vec![1; nfiles];
                    }
                }
                let mut params = json!({ "torrentPath": file, "filePriorities": priorities });
                if let Some(d) = dir {
                    params["outDir"] = json!(d);
                }
                if let Some(l) = limit {
                    let bps = parse_bytes(l).ok_or_else(|| format!("bad limit '{}'", l))?;
                    params["rateLimitBps"] = json!(bps);
                }
                let r = c
                    .call("downloads.addTorrent", params)
                    .map_err(|e| e.to_string())?;
                Ok(format!("added torrent #{} ({})", r["id"].as_u64().unwrap_or(0), info["name"].as_str().unwrap_or("?")))
            }
            Commands::Magnet { uri, dir, files, limit } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                let _ = files; // file selection needs metadata; use the GUI
                let mut params = json!({ "magnet": uri });
                if let Some(d) = dir {
                    params["outDir"] = json!(d);
                }
                if let Some(l) = limit {
                    let bps = parse_bytes(l).ok_or_else(|| format!("bad limit '{}'", l))?;
                    params["rateLimitBps"] = json!(bps);
                }
                let r = c
                    .call("downloads.addTorrent", params)
                    .map_err(|e| e.to_string())?;
                Ok(format!("added magnet #{}", r["id"].as_u64().unwrap_or(0)))
            }
            Commands::TorrentCreate { input, announce, piece_len, output } => {
                let path = std::path::Path::new(input);
                let meta_input = std::fs::metadata(path).map_err(|e| e.to_string())?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "bundle".into());
                let mut inputs = Vec::new();
                if meta_input.is_dir() {
                    collect_files(path, &name, &mut inputs, 0)
                        .map_err(|e| e.to_string())?;
                } else {
                    inputs.push(vdm_core::bt::metainfo::CreateInput {
                        path: vec![name.clone()],
                        fs_path: path.to_path_buf(),
                    });
                }
                if inputs.is_empty() {
                    return Err("no input files".into());
                }
                let total: u64 = inputs
                    .iter()
                    .map(|i| std::fs::metadata(&i.fs_path).map(|m| m.len()).unwrap_or(0))
                    .sum();
                let plen = piece_len.unwrap_or_else(|| vdm_core::bt::metainfo::suggest_piece_len(total));
                let (meta, bytes) = vdm_core::bt::metainfo::create_torrent(
                    &name,
                    &inputs,
                    plen,
                    announce.as_deref(),
                    &[],
                    false,
                )
                .map_err(|e| e.to_string())?;
                let out_path = output
                    .clone()
                    .unwrap_or_else(|| format!("{}.torrent", input.trim_end_matches('/')));
                std::fs::write(&out_path, &bytes).map_err(|e| e.to_string())?;
                Ok(format!(
                    "wrote {} ({} files, {}, {} pieces, info hash {})",
                    out_path,
                    inputs.len(),
                    format_bytes(meta.total_len),
                    meta.piece_count(),
                    hex::encode(meta.info_hash)
                ))
            }
            Commands::Seed { file, port } => {
                let path = std::path::Path::new(file);
                if !path.is_file() {
                    return Err(format!("{} is not a file", file));
                }
                let tracker = vdm_core::bt::tracker::TrackerServer::start(&format!("127.0.0.1:{}", port))
                    .map_err(|e| e.to_string())?;
                let (seeder, _torrent, meta) =
                    vdm_core::bt::seeder::Seeder::seed_file(path, &tracker.url())
                        .map_err(|e| e.to_string())?;
                tracker.register(meta.info_hash, seeder.addr);
                let magnet = vdm_core::bt::magnet::Magnet::from_parts(
                    meta.info_hash,
                    Some(meta.name.clone()),
                    vec![tracker.url()],
                );
                println!("seeding {} at {}", meta.name, seeder.addr);
                println!("tracker: {}", tracker.url());
                println!("magnet:  {}", magnet.to_uri());
                println!("press Ctrl-C to stop");
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
            Commands::Limits { global } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                match global {
                    Some(g) => {
                        let bps = if g == "0" || g.eq_ignore_ascii_case("none") {
                            0
                        } else {
                            parse_bytes(g).ok_or_else(|| format!("bad limit '{}'", g))?
                        };
                        c.call("limits.set", json!({ "globalBps": bps }))
                            .map_err(|e| e.to_string())?;
                        Ok(if bps == 0 {
                            "global limit disabled".to_string()
                        } else {
                            format!("global limit set to {}/s", format_bytes(bps))
                        })
                    }
                    None => {
                        let l = c.call("limits.get", json!({})).map_err(|e| e.to_string())?;
                        let bps = l["globalBps"].as_u64().unwrap_or(0);
                        Ok(if bps == 0 {
                            "global limit: unlimited".to_string()
                        } else {
                            format!("global limit: {}/s", format_bytes(bps))
                        })
                    }
                }
            }
            Commands::Schedule {
                stop_start,
                stop_end,
                clear,
                max_active,
                shutdown_when_done,
                on_drained,
            } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                let mut params = json!({});
                if *clear {
                    params["stopWindow"] = json!([]);
                } else if let (Some(a), Some(b)) = (stop_start, stop_end) {
                    for t in [a, b] {
                        if vdm_core::engine::scheduler::parse_hhmm(t).is_none() {
                            return Err(format!("bad time '{}' (use HH:MM)", t));
                        }
                    }
                    params["stopWindow"] = json!([a, b]);
                }
                if let Some(m) = max_active {
                    params["maxActive"] = json!(m);
                }
                if *shutdown_when_done {
                    params["shutdownWhenDone"] = json!(true);
                }
                if let Some(cmd) = on_drained {
                    params["onQueueDrained"] = json!(cmd);
                }
                if params.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                    c.call("queue.set", params).map_err(|e| e.to_string())?;
                }
                let q = c.call("queue.get", json!({})).map_err(|e| e.to_string())?;
                Ok(format!(
                    "scheduler: maxActive={}, stopWindow={:?}, shutdownWhenDone={}",
                    q["maxActive"].as_u64().unwrap_or(0),
                    q["stopWindow"],
                    q["shutdownWhenDone"].as_bool().unwrap_or(false)
                ))
            }
            Commands::Settings { dir, conn, proxy, user_agent } => {
                ensure_daemon(addr)?;
                let c = client(addr);
                let mut params = json!({});
                if let Some(d) = dir {
                    params["downloadDir"] = json!(d);
                }
                if let Some(n) = conn {
                    params["defaultConnections"] = json!(n);
                }
                if let Some(p) = proxy {
                    if p.eq_ignore_ascii_case("none") {
                        params["proxy"] = Value::Null;
                    } else {
                        let parsed = vdm_core::proxy::ProxyConfig::parse_url(p)
                            .map_err(|e| e.to_string())?;
                        params["proxy"] = serde_json::to_value(parsed).unwrap();
                    }
                }
                if let Some(ua) = user_agent {
                    params["userAgent"] = json!(ua);
                }
                if params.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                    c.call("settings.set", params).map_err(|e| e.to_string())?;
                }
                let s = c.call("settings.get", json!({})).map_err(|e| e.to_string())?;
                Ok(serde_json::to_string_pretty(&s).unwrap())
            }
            Commands::Version => {
                Ok(format!(
                    "VelocityDM {} (engine {}, features: http, hls+aes128, bittorrent, magnet, batch, scheduler, proxy, refresh-link)",
                    env!("CARGO_PKG_VERSION"),
                    vdm_core::VERSION
                ))
            }
            Commands::Daemon => unreachable!("handled in main"),
            #[cfg(feature = "gui")]
            Commands::Gui => unreachable!("handled in main"),
        }
    }
}

fn collect_files(
    dir: &std::path::Path,
    root_name: &str,
    out: &mut Vec<vdm_core::bt::metainfo::CreateInput>,
    depth: usize,
) -> std::io::Result<()> {
    if depth > 16 {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, root_name, out, depth + 1)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(dir.parent().unwrap_or(dir))
                .unwrap_or(&path);
            let _ = root_name;
            let components: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();
            // drop the root directory component from paths inside the torrent
            let inner: Vec<String> = if components.len() > 1 {
                components[1..].to_vec()
            } else {
                components
            };
            out.push(vdm_core::bt::metainfo::CreateInput {
                path: inner,
                fs_path: path.clone(),
            });
        }
    }
    Ok(())
}

fn render_list(list: &Value) -> String {
    let arr = match list.as_array() {
        Some(a) => a,
        None => return String::new(),
    };
    if arr.is_empty() {
        return "no downloads".to_string();
    }
    let mut out = String::new();
    out.push_str(&format!(
        "{:<4} {:<10} {:<34} {:>10} {:>10} {:>9}  {}\n",
        "ID", "STATE", "NAME", "DONE", "SIZE", "SPEED", "ETA"
    ));
    for t in arr {
        let id = t["id"].as_u64().unwrap_or(0);
        let state = t["state"].as_str().unwrap_or("?");
        let name = {
            let n = t["name"].as_str().unwrap_or("?");
            if n.chars().count() > 32 {
                n.chars().take(29).collect::<String>() + "..."
            } else {
                n.to_string()
            }
        };
        let done = t["done"].as_u64().unwrap_or(0);
        let size = t["size"].as_u64().unwrap_or(0);
        let speed = t["speed"].as_u64().unwrap_or(0);
        let eta = t["etaSecs"].as_u64().unwrap_or(0);
        let eta_s = if eta > 0 && state == "downloading" {
            format!("{}s", eta)
        } else {
            "-".to_string()
        };
        let peers = t["peers"].as_u64().unwrap_or(0);
        let state_disp = if peers > 0 {
            format!("{} p{}", state, peers)
        } else {
            state.to_string()
        };
        out.push_str(&format!(
            "{:<4} {:<10} {:<34} {:>10} {:>10} {:>9}  {}\n",
            id,
            state_disp,
            name,
            format_bytes(done),
            if size > 0 {
                format_bytes(size)
            } else {
                "?".to_string()
            },
            if speed > 0 {
                format_bytes(speed) + "/s"
            } else {
                "-".to_string()
            },
            eta_s
        ));
    }
    out
}

#[allow(dead_code)]
fn unused_now() -> u64 {
    now_ms()
}
