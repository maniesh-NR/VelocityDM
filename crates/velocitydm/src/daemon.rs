//! Daemon mode: engine + RPC server + config persistence + lifecycle.

use std::sync::atomic::{AtomicBool, Ordering};
use vdm_core::engine::{Engine, EngineConfig};
use vdm_rpc::RpcServer;

/// Config file location: portable mode keeps it beside the binary,
/// otherwise $XDG_CONFIG_HOME/velocitydm/config.json.
pub fn config_path(portable: bool) -> std::path::PathBuf {
    if portable {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join("velocitydm.config.json");
            }
        }
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    base.join("velocitydm").join("config.json")
}

pub fn load_config(portable: bool) -> EngineConfig {
    let path = config_path(portable);
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<EngineConfig>(&text) {
            return cfg;
        }
    }
    EngineConfig::default()
}

pub fn save_config(cfg: &EngineConfig, portable: bool) {
    let path = config_path(portable);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, text);
    }
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

pub fn run(addr: &str, portable: bool) -> i32 {
    let cfg = load_config(portable);
    let engine = Engine::new(cfg);
    let server = match RpcServer::start(engine.clone(), addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot bind {}: {} (is another daemon running?)", addr, e);
            return 1;
        }
    };
    println!(
        "VelocityDM daemon {} listening on http://{} (config: {})",
        env!("CARGO_PKG_VERSION"),
        server.addr,
        config_path(portable).display()
    );
    // persist settings when they change
    {
        let engine = engine.clone();
        let portable_c = portable;
        let rx = engine.subscribe();
        std::thread::spawn(move || {
            while let Ok(evt) = rx.recv() {
                match evt {
                    vdm_core::engine::events::Event::ConfigChanged => {
                        save_config(&engine.config(), portable_c);
                    }
                    vdm_core::engine::events::Event::QueueDrained => {
                        let cfg = engine.config();
                        if cfg.scheduler.shutdown_when_done {
                            eprintln!("queue drained: shutting down (shutdown-when-done)");
                            SHUTDOWN.store(true, Ordering::SeqCst);
                        }
                    }
                    _ => {}
                }
            }
        });
    }
    // block until shutdown: requested locally (GUI close, Ctrl-C) or over RPC
    // (daemon.shutdown -> the RPC server winds itself down)
    while !SHUTDOWN.load(Ordering::SeqCst) && !server.stopped() {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    server.stop();
    engine.shutdown();
    0
}

/// Request daemon exit (used by the GUI close handler).
pub fn request_shutdown() {
    SHUTDOWN.store(true, Ordering::SeqCst);
}
