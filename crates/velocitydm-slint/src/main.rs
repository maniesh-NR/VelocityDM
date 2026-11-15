//! VelocityDM Linux desktop frontend — Slint.
//!
//! The "modern Linux" stack from the platform matrix: this binary
//! embeds the shared Rust core in-process (engine + JSON-RPC server,
//! exactly like the egui GUI and the daemon), renders Material You
//! chrome from the shared `vdm-md3` token engine, and uses the
//! toolkit's native input widgets for platform-standard interaction.
//! Talks to the SAME config.json / ui.json as the other frontends.

mod config;

use config::UiPrefs;
use slint::{ModelRc, SharedString, VecModel};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;
use vdm_core::engine::events::TaskState;
use vdm_core::engine::{Engine, EngineConfig, HttpOptions};
use vdm_core::proxy::ProxyConfig;
use vdm_core::util::format_bytes;
use vdm_rpc::RpcServer;

slint::include_modules!();

fn color32(hex: u32) -> slint::Color {
    slint::Color::from_rgb_u8(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

/// Push a vdm-md3 scheme into the Theme global.
fn apply_theme(ui: &AppWindow, prefs: &UiPrefs) {
    let s = vdm_md3::Scheme::from_seed_hex(prefs.seed, prefs.dark);
    let t: Theme = ui.global::<Theme>();
    t.set_primary(color32(s.primary));
    t.set_on_primary(color32(s.on_primary));
    t.set_primary_container(color32(s.primary_container));
    t.set_on_primary_container(color32(s.on_primary_container));
    t.set_secondary_container(color32(s.secondary_container));
    t.set_on_secondary_container(color32(s.on_secondary_container));
    t.set_tertiary(color32(s.tertiary));
    t.set_error(color32(s.error));
    t.set_on_error(color32(s.on_error));
    t.set_surface(color32(s.surface));
    t.set_surface_container_lowest(color32(s.surface_container_lowest));
    t.set_surface_container_low(color32(s.surface_container_low));
    t.set_surface_container(color32(s.surface_container));
    t.set_surface_container_high(color32(s.surface_container_high));
    t.set_surface_container_highest(color32(s.surface_container_highest));
    t.set_on_surface(color32(s.on_surface));
    t.set_on_surface_variant(color32(s.on_surface_variant));
    t.set_outline(color32(s.outline));
    t.set_outline_variant(color32(s.outline_variant));
    ui.set_theme_dark(prefs.dark);
    ui.set_theme_seed_label(SharedString::from(vdm_md3::seed_name(prefs.seed)));
}

fn state_chip_by(
    primary: slint::Color,
    tertiary: slint::Color,
    error: slint::Color,
    on_surface_variant: slint::Color,
    on_secondary_container: slint::Color,
    state: TaskState,
) -> (&'static str, slint::Color) {
    match state {
        TaskState::Completed => ("done", tertiary),
        TaskState::Failed => ("failed", error),
        TaskState::Paused | TaskState::Queued => ("queued", on_surface_variant),
        TaskState::Downloading => ("active", primary),
        TaskState::Verifying => ("verify", on_secondary_container),
        TaskState::Probing => ("probe", on_secondary_container),
        TaskState::Seeding => ("seed", primary),
    }
}

/// Fill the settings panel properties from an EngineConfig.
fn push_settings(ui: &AppWindow, cfg: &EngineConfig) {
    ui.set_settings_dir(SharedString::from(
        cfg.download_dir.to_string_lossy().to_string(),
    ));
    ui.set_settings_connections(cfg.default_connections as i32);
    ui.set_settings_max_active(cfg.scheduler.max_active as i32);
    ui.set_settings_limit(SharedString::from(if cfg.global_limit_bps > 0 {
        format_bytes(cfg.global_limit_bps).to_string()
    } else {
        String::new()
    }));
    ui.set_settings_proxy(SharedString::from(
        cfg.proxy.as_ref().map(proxy_display).unwrap_or_default(),
    ));
    let (start, end) = cfg
        .scheduler
        .stop_window
        .as_ref()
        .map(|w| (w.0.clone(), w.1.clone()))
        .unwrap_or_default();
    ui.set_settings_stop_start(SharedString::from(start));
    ui.set_settings_stop_end(SharedString::from(end));
    ui.set_settings_shutdown(cfg.scheduler.shutdown_when_done);
}

fn proxy_display(p: &ProxyConfig) -> String {
    match p {
        ProxyConfig::Http { host, port, .. } => format!("http://{host}:{port}"),
        ProxyConfig::Socks5 { host, port, .. } => format!("socks5://{host}:{port}"),
    }
}

fn main() -> ExitCode {
    let portable = std::env::args().any(|a| a == "--portable");
    let addr = "127.0.0.1:36990";
    let cfg = config::load_config(portable);
    let engine = Engine::new(cfg.clone());
    let server = match RpcServer::start(engine.clone(), addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot bind {addr} ({e}); is another VelocityDM frontend running?");
            return ExitCode::FAILURE;
        }
    };
    let prefs = config::load_prefs(portable);

    let ui = match AppWindow::new() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("cannot open window: {e}");
            return ExitCode::FAILURE;
        }
    };
    apply_theme(&ui, &prefs);
    push_settings(&ui, &cfg);

    let model = Rc::new(VecModel::<TaskItem>::default());
    ui.set_tasks(ModelRc::from(model.clone()));

    // ---- periodic refresh (engine snapshots -> UI model) ----------
    // Chip colors come from the live Theme global so a Material You
    // seed change restyles the list on the next tick.
    let engine_r = engine.clone();
    let weak = ui.as_weak();
    let model_r = model.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(200),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let snaps = engine_r.list();
            let total: u64 = snaps.iter().map(|s| s.speed).sum();
            ui.set_total_speed(SharedString::from(if total > 0 {
                format!("{}/s total", format_bytes(total))
            } else {
                "idle".to_string()
            }));
            ui.set_empty(snaps.is_empty());
            let t: Theme = ui.global::<Theme>();
            let rows: Vec<TaskItem> = snaps
                .iter()
                .map(|s| {
                    let (chip_text, chip_color) = state_chip_by(
                        t.get_primary(),
                        t.get_tertiary(),
                        t.get_error(),
                        t.get_on_surface_variant(),
                        t.get_on_secondary_container(),
                        s.state,
                    );
                    let eta = if s.eta_secs > 0 && s.state == TaskState::Downloading {
                        format!("{}s", s.eta_secs)
                    } else {
                        "-".to_string()
                    };
                    let extra = if s.peers > 0 {
                        format!(" {}p/{}s", s.peers, s.seeds)
                    } else {
                        String::new()
                    };
                    let speed = if s.speed > 0 {
                        format!("{}/s", format_bytes(s.speed))
                    } else {
                        "-".to_string()
                    };
                    let stats = format!(
                        "{} / {}  {}  {}{}",
                        format_bytes(s.done),
                        if s.size > 0 {
                            format_bytes(s.size)
                        } else {
                            "?".to_string()
                        },
                        speed,
                        eta,
                        extra
                    );
                    TaskItem {
                        id: s.id as i32,
                        name: SharedString::from(s.name.clone()),
                        state_text: SharedString::from(chip_text.to_string()),
                        state_color: chip_color,
                        stats: SharedString::from(stats),
                        detail: SharedString::from(if s.out_path.is_empty() {
                            String::new()
                        } else {
                            s.out_path.clone()
                        }),
                        error_text: SharedString::from(s.error.clone().unwrap_or_default()),
                        progress: if s.size > 0 {
                            (s.done as f32 / s.size as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        },
                        can_pause: matches!(
                            s.state,
                            TaskState::Downloading | TaskState::Probing | TaskState::Queued
                        ),
                        can_resume: matches!(s.state, TaskState::Paused | TaskState::Failed),
                    }
                })
                .collect();
            model_r.set_vec(rows);
        },
    );

    // ---- add download ---------------------------------------------
    let engine_a = engine.clone();
    let weak_a = ui.as_weak();
    ui.on_add_download(move || {
        let Some(ui) = weak_a.upgrade() else { return };
        let url = ui.get_add_url().to_string();
        let url = url.trim().to_string();
        if url.is_empty() {
            return;
        }
        let limit = vdm_core::util::parse_bytes(ui.get_add_limit().as_str()).unwrap_or(0);
        let quality = ui.get_add_quality().to_string();
        let mut err = None;
        if url.starts_with("magnet:") {
            match vdm_core::bt::Magnet::parse(&url) {
                Ok(m) => {
                    let _ =
                        engine_a.add_torrent(vdm_core::bt::BtSpec::Magnet(m), None, vec![], limit);
                }
                Err(e) => err = Some(format!("! {e}")),
            }
        } else if url.ends_with(".torrent") || std::path::Path::new(&url).exists() {
            match std::fs::read(&url) {
                Ok(bytes) => {
                    let _ = engine_a.add_torrent(
                        vdm_core::bt::BtSpec::TorrentBytes(bytes),
                        None,
                        vec![],
                        limit,
                    );
                }
                Err(e) => err = Some(format!("! read: {e}")),
            }
        } else {
            let opts = HttpOptions {
                quality: quality.trim().to_string(),
                rate_limit_bps: limit,
                ..Default::default()
            };
            if let Err(e) = engine_a.add_download(&url, opts) {
                err = Some(format!("! {e}"));
            }
        }
        match err {
            Some(e) => ui.set_add_url(SharedString::from(e)),
            None => ui.set_add_url(SharedString::from("")),
        }
    });

    // ---- per-task actions -------------------------------------------
    {
        let e = engine.clone();
        ui.on_pause_task(move |id| {
            let _ = e.pause(id as u64);
        });
    }
    {
        let e = engine.clone();
        ui.on_resume_task(move |id| {
            let _ = e.resume(id as u64);
        });
    }
    {
        let e = engine.clone();
        ui.on_remove_task(move |id| {
            let _ = e.remove(id as u64, false);
        });
    }

    // ---- settings apply ---------------------------------------------
    {
        let engine_s = engine.clone();
        let portable_s = portable;
        let weak_s = ui.as_weak();
        ui.on_apply_settings(move || {
            let Some(ui) = weak_s.upgrade() else { return };
            // Start from the live config so fields this panel does not
            // expose (user_agent, scheduler details) survive Apply.
            let mut cfg = engine_s.config();
            let dir = ui.get_settings_dir().to_string();
            if !dir.trim().is_empty() {
                cfg.download_dir = std::path::PathBuf::from(dir);
            }
            cfg.default_connections = ui.get_settings_connections().clamp(1, 32) as usize;
            cfg.scheduler.max_active = ui.get_settings_max_active().clamp(1, 16) as usize;
            cfg.global_limit_bps =
                vdm_core::util::parse_bytes(ui.get_settings_limit().as_str()).unwrap_or(0);
            let proxy = ui.get_settings_proxy().to_string();
            let proxy = proxy.trim().to_string();
            if proxy.is_empty() || proxy.eq_ignore_ascii_case("none") {
                cfg.proxy = None;
            } else if let Ok(p) = ProxyConfig::parse_url(&proxy) {
                cfg.proxy = Some(p);
            }
            let start = ui.get_settings_stop_start().to_string();
            let end = ui.get_settings_stop_end().to_string();
            if vdm_core::engine::scheduler::parse_hhmm(&start).is_some()
                && vdm_core::engine::scheduler::parse_hhmm(&end).is_some()
            {
                cfg.scheduler.stop_window = Some((start, end));
            }
            cfg.scheduler.shutdown_when_done = ui.get_settings_shutdown();
            engine_s.set_config(cfg.clone());
            config::save_config(&cfg, portable_s);
            push_settings(&ui, &cfg);
        });
    }

    // ---- Material You controls ---------------------------------------
    {
        let portable_t = portable;
        let weak_t = ui.as_weak();
        ui.on_cycle_theme(move || {
            let Some(ui) = weak_t.upgrade() else { return };
            let mut prefs = config::load_prefs(portable_t);
            let idx = vdm_md3::SEEDS
                .iter()
                .position(|(s, _)| *s == prefs.seed)
                .unwrap_or(0);
            let (next_seed, _) = vdm_md3::SEEDS[(idx + 1) % vdm_md3::SEEDS.len()];
            prefs.seed = next_seed;
            config::save_prefs(&prefs, portable_t);
            apply_theme(&ui, &prefs);
        });
    }
    {
        let portable_t = portable;
        let weak_t = ui.as_weak();
        ui.on_toggle_dark(move || {
            let Some(ui) = weak_t.upgrade() else { return };
            let mut prefs = config::load_prefs(portable_t);
            prefs.dark = !prefs.dark;
            config::save_prefs(&prefs, portable_t);
            apply_theme(&ui, &prefs);
        });
    }

    // ---- run ----------------------------------------------------------
    let result = ui.run();
    drop(timer); // keep the refresh timer alive for the whole run
    engine.shutdown();
    server.stop();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("window error: {e}");
            ExitCode::FAILURE
        }
    }
}
