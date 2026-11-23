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
            // S11-i B2p: validate the connection preset at the NMH boundary
            // too (the engine re-validates; the extension gets our message
            // immediately)
            if let Err(e) = apply_conn_preset(&msg, &mut options) {
                return json!({ "ok": false, "error": e });
            }
            // S11-i B16: validate the browser-reported proxy shape and
            // forward it as the add options' browserProxy (scheme in
            // http/https/socks5 — https maps to the engine's Http class
            // (CONNECT-tunneled TLS); host <= 253 printable chars; port
            // 1..=65535)
            match browser_proxy_from_msg(&msg) {
                Ok(Some(bp)) => options["browserProxy"] = bp,
                Ok(None) => {}
                Err(e) => return json!({ "ok": false, "error": e }),
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
                // expand a raw list by adding each URL; S11-i B5: an
                // optional `cookiesBy` map carries per-URL cookie jars
                // (built by the extension, capped at the first 32 URLs of
                // the batch — re-capped here defensively; non-string
                // entries dropped) so authenticated batch adds ride the
                // page session like single adds do
                let jars = cookies_by_from_msg(&msg);
                let mut ids = Vec::new();
                for u in list.iter().filter_map(|x| x.as_str()) {
                    let mut params = json!({ "url": u });
                    if let Some(jar) = jars.get(u) {
                        if !jar.is_empty() {
                            params["options"] = json!({ "headers": [["Cookie", jar]] });
                        }
                    }
                    if let Ok(r) = call_daemon("downloads.add", params) {
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
                std::env::var("VELOCITYDM_BIN")
                    .ok()
                    .map(std::path::PathBuf::from)
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
        std::env::var("VELOCITYDM_BIN")
            .ok()
            .map(std::path::PathBuf::from)
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

/// S11-i B2p: validate the frame's `connPreset` (if present) and forward
/// the normalized value into the add options; an unknown preset is
/// rejected with our message.
fn apply_conn_preset(msg: &Value, options: &mut Value) -> Result<(), String> {
    if let Some(p) = msg.get("connPreset").and_then(|p| p.as_str()) {
        let valid = vdm_core::engine::task::validate_conn_preset(p).map_err(|e| e.to_string())?;
        options["connPreset"] = json!(valid);
    }
    Ok(())
}

/// S11-i B16: validate the frame's `browserProxy` (if present) into the
/// engine's add-option shape `{"type":"http"|"socks5","host","port"}`.
/// `None` = no proxy reported (or an explicit JSON null) — the add proceeds
/// unproxied; a malformed shape is rejected with our message.
fn browser_proxy_from_msg(msg: &Value) -> Result<Option<Value>, String> {
    let Some(v) = msg.get("browserProxy") else {
        return Ok(None);
    };
    if v.is_null() {
        return Ok(None);
    }
    let obj = v
        .as_object()
        .ok_or_else(|| "browserProxy must be an object".to_string())?;
    let scheme = obj.get("scheme").and_then(|s| s.as_str()).unwrap_or("");
    let host = obj.get("host").and_then(|h| h.as_str()).unwrap_or("");
    let port = obj.get("port").and_then(|p| p.as_u64()).unwrap_or(0);
    let kind = match scheme {
        "http" | "https" => "http",
        "socks5" => "socks5",
        other => {
            return Err(format!("browserProxy: unsupported scheme '{}'", other));
        }
    };
    if host.is_empty() || host.len() > 253 || !host.chars().all(|c| c.is_ascii_graphic()) {
        return Err("browserProxy: bad host".into());
    }
    if !(1..=65535).contains(&port) {
        return Err("browserProxy: bad port".into());
    }
    Ok(Some(json!({ "type": kind, "host": host, "port": port })))
}

/// S11-i B5: the per-URL cookie map for URL-list batches — at most the
/// first 32 entries, string values only (anything else is dropped, never
/// an error: the batch still downloads).
fn cookies_by_from_msg(msg: &Value) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Some(obj) = msg.get("cookiesBy").and_then(|v| v.as_object()) else {
        return out;
    };
    for (k, v) in obj.iter().take(32) {
        if let Some(s) = v.as_str() {
            out.insert(k.clone(), s.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// S11-i B2p: the NMH boundary validates the connection preset and
    /// forwards the normalized value into the add options.
    #[test]
    fn conn_preset_validated_at_nmh_boundary() {
        let mut options = json!({});
        apply_conn_preset(&json!({ "connPreset": "HIGH" }), &mut options).unwrap();
        assert_eq!(options["connPreset"], "high");
        // a preset-free frame leaves the options untouched
        let mut bare = json!({ "filename": "f.zip" });
        apply_conn_preset(&json!({}), &mut bare).unwrap();
        assert!(bare.get("connPreset").is_none());
        // an unknown preset is rejected with our message
        let err = apply_conn_preset(&json!({ "connPreset": "turbo" }), &mut json!({})).unwrap_err();
        assert!(err.contains("unknown connection preset"));
    }

    #[test]
    fn conn_preset_accepts_all_classes() {
        for ok in ["auto", "high", "medium", "low"] {
            let mut options = json!({});
            apply_conn_preset(&json!({ "connPreset": ok }), &mut options).unwrap();
            assert_eq!(options["connPreset"], ok);
        }
    }

    /// S11-i B16: the browser-proxy shape is validated at the NMH boundary.
    #[test]
    fn browser_proxy_shape_validated_at_nmh_boundary() {
        // a well-formed fixed proxy forwards as the engine's option shape
        let bp = browser_proxy_from_msg(&json!({
            "browserProxy": { "scheme": "socks5", "host": "127.0.0.1", "port": 1080 }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(
            bp,
            json!({ "type": "socks5", "host": "127.0.0.1", "port": 1080 })
        );
        // https maps onto the engine's Http class
        let https = browser_proxy_from_msg(&json!({
            "browserProxy": { "scheme": "https", "host": "proxy.corp", "port": 3129 }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(https["type"], "http");
        // absent / null -> no proxy
        assert_eq!(browser_proxy_from_msg(&json!({})).unwrap(), None);
        assert_eq!(
            browser_proxy_from_msg(&json!({ "browserProxy": null })).unwrap(),
            None
        );
        // bad shapes rejected: scheme, host, port, type
        assert!(browser_proxy_from_msg(&json!({ "browserProxy": "socks5://x:1" })).is_err());
        assert!(browser_proxy_from_msg(
            &json!({ "browserProxy": { "scheme": "ftp", "host": "x", "port": 1 } })
        )
        .is_err());
        assert!(browser_proxy_from_msg(
            &json!({ "browserProxy": { "scheme": "http", "host": "", "port": 1 } })
        )
        .is_err());
        let long_host = "h".repeat(254);
        assert!(browser_proxy_from_msg(
            &json!({ "browserProxy": { "scheme": "http", "host": long_host, "port": 1 } })
        )
        .is_err());
        assert!(browser_proxy_from_msg(
            &json!({ "browserProxy": { "scheme": "http", "host": "x", "port": 0 } })
        )
        .is_err());
        assert!(browser_proxy_from_msg(
            &json!({ "browserProxy": { "scheme": "http", "host": "x", "port": 70000 } })
        )
        .is_err());
    }

    /// S11-i B5: the per-URL cookie map for URL-list batches — capped at
    /// the first 32 entries, string values only, absent/negative shapes
    /// tolerated (the batch still downloads).
    #[test]
    fn cookies_by_shape_at_nmh_boundary() {
        // well-formed map forwards verbatim
        let jars = cookies_by_from_msg(&json!({
            "cookiesBy": { "https://a/1": "s=1; p=2", "https://b/2": "" }
        }));
        assert_eq!(
            jars.get("https://a/1").map(|s| s.as_str()),
            Some("s=1; p=2")
        );
        assert_eq!(jars.get("https://b/2").map(|s| s.as_str()), Some(""));

        // absent / null / non-object -> empty (today's behavior)
        assert!(cookies_by_from_msg(&json!({})).is_empty());
        assert!(cookies_by_from_msg(&json!({ "cookiesBy": null })).is_empty());
        assert!(cookies_by_from_msg(&json!({ "cookiesBy": ["a"] })).is_empty());

        // non-string values are dropped, never an error
        let jars = cookies_by_from_msg(&json!({
            "cookiesBy": { "https://a/1": "s=1", "https://a/2": 7, "https://a/3": null }
        }));
        assert_eq!(jars.len(), 1);

        // the first-32 cap holds even when more entries ride the frame
        // (serde_json's Map iterates in sorted key order — the assertion is
        // order-independent: exactly 32 of the 40 entries survive)
        let many: serde_json::Map<String, Value> = (0..40)
            .map(|i| (format!("https://x/{}", i), json!(format!("j{}", i))))
            .collect();
        let jars = cookies_by_from_msg(&json!({ "cookiesBy": many }));
        assert_eq!(jars.len(), 32);
        assert_eq!(jars.get("https://x/0").map(|s| s.as_str()), Some("j0"));
        let missing: Vec<_> = (0..40)
            .filter(|i| !jars.contains_key(&format!("https://x/{}", i)))
            .collect();
        assert_eq!(
            missing.len(),
            8,
            "exactly 8 entries dropped by the cap: {:?}",
            missing
        );
    }
}
