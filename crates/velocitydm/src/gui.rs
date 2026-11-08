//! Material You (Material Design 3) egui interface (in-process engine + RPC
//! server). All colors come from the MD3 scheme generated from the user's
//! seed color — see [`crate::theme`].

use crate::daemon::{load_config, request_shutdown, save_config};
use crate::theme::{self, Scheme, UiPrefs};
use eframe::egui;
use eframe::egui::{Align, Color32, Layout, RichText, Rounding, Slider, Stroke, TextEdit, Vec2};
use std::sync::Arc;
use vdm_core::engine::events::TaskState;
use vdm_core::engine::{Engine, EngineConfig, HttpOptions};
use vdm_core::proxy::ProxyConfig;
use vdm_core::util::format_bytes;
use vdm_rpc::RpcServer;

#[derive(Clone)]
struct TaskRow {
    id: u64,
    state: String,
    is_state: TaskState,
    name: String,
    out_path: String,
    done: u64,
    size: u64,
    speed: u64,
    eta_secs: u64,
    peers: u32,
    seeds: u32,
    error: Option<String>,
}

pub struct VdmApp {
    engine: Arc<Engine>,
    _server: Arc<RpcServer>,
    rows: Vec<TaskRow>,
    add_url: String,
    add_quality: String,
    add_limit: String,
    add_filename: String,
    show_settings: bool,
    settings: EngineConfig,
    proxy_text: String,
    stop_start: String,
    stop_end: String,
    last_tick: std::time::Instant,
    total_speed: u64,
    selected_task: Option<u64>,
    portable: bool,
    prefs: UiPrefs,
    scheme: Scheme,
}

pub fn run(addr: &str, portable: bool) -> i32 {
    let cfg = load_config(portable);
    let engine = Engine::new(cfg);
    let server = match RpcServer::start(engine.clone(), addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "cannot bind {}: {} (is the daemon already running?)",
                addr, e
            );
            return 1;
        }
    };
    let prefs = theme::load_prefs(portable);
    let scheme = Scheme::from_seed_hex(prefs.seed, prefs.dark);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 640.0])
            .with_min_inner_size([760.0, 480.0])
            .with_title("VelocityDM"),
        ..Default::default()
    };
    let engine_c = engine.clone();
    let _server_c = server.clone();
    let result = eframe::run_native(
        "VelocityDM",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, &scheme);
            Box::new(VdmApp::new(engine_c, _server_c, portable, prefs, scheme))
        }),
    );
    engine.shutdown();
    server.stop();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gui error: {}", e);
            1
        }
    }
}

/// MD3 state chip (rounded container + label).
fn chip(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32, outline: Color32) {
    egui::Frame::default()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, outline))
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(fg).strong());
        });
}

/// Filled button (MD3: primary container, full rounding).
fn filled_button<'a>(text: &'a str, s: &'a Scheme) -> egui::Button<'a> {
    egui::Button::new(RichText::new(text).color(s.on_primary))
        .fill(s.primary)
        .rounding(Rounding::same(9999.0))
        .min_size(Vec2::new(0.0, 32.0))
}

/// Tonal button (MD3: secondary container).
fn tonal_button<'a>(text: &'a str, s: &'a Scheme) -> egui::Button<'a> {
    egui::Button::new(RichText::new(text).color(s.on_secondary_container))
        .fill(s.secondary_container)
        .rounding(Rounding::same(9999.0))
        .min_size(Vec2::new(0.0, 32.0))
}

/// Error-toned text button.
fn error_button<'a>(text: &'a str, s: &'a Scheme) -> egui::Button<'a> {
    egui::Button::new(RichText::new(text).color(s.error))
        .fill(Color32::TRANSPARENT)
        .rounding(Rounding::same(9999.0))
        .min_size(Vec2::new(0.0, 32.0))
}

impl VdmApp {
    fn new(
        engine: Arc<Engine>,
        server: Arc<RpcServer>,
        portable: bool,
        prefs: UiPrefs,
        scheme: Scheme,
    ) -> VdmApp {
        let settings = engine.config();
        let proxy_text = settings
            .proxy
            .as_ref()
            .map(proxy_display)
            .unwrap_or_default();
        let stop_start = settings
            .scheduler
            .stop_window
            .as_ref()
            .map(|w| w.0.clone())
            .unwrap_or_default();
        let stop_end = settings
            .scheduler
            .stop_window
            .as_ref()
            .map(|w| w.1.clone())
            .unwrap_or_default();
        VdmApp {
            engine,
            _server: server,
            rows: Vec::new(),
            add_url: String::new(),
            add_quality: "best".into(),
            add_limit: String::new(),
            add_filename: String::new(),
            show_settings: false,
            settings,
            proxy_text,
            stop_start,
            stop_end,
            last_tick: std::time::Instant::now(),
            total_speed: 0,
            selected_task: None,
            portable,
            prefs,
            scheme,
        }
    }

    fn refresh(&mut self) {
        let snaps = self.engine.list();
        self.total_speed = snaps.iter().map(|s| s.speed).sum();
        self.rows = snaps
            .into_iter()
            .map(|s| TaskRow {
                id: s.id,
                state: s.state.as_str().to_string(),
                is_state: s.state,
                name: s.name.clone(),
                out_path: s.out_path.clone(),
                done: s.done,
                size: s.size,
                speed: s.speed,
                eta_secs: s.eta_secs,
                peers: s.peers,
                seeds: s.seeds,
                error: s.error.clone(),
            })
            .collect();
    }

    fn set_theme(&mut self, ctx: &egui::Context, seed: u32, dark: bool) {
        self.prefs.seed = seed;
        self.prefs.dark = dark;
        self.scheme = Scheme::from_seed_hex(seed, dark);
        theme::apply(ctx, &self.scheme);
        theme::save_prefs(&self.prefs, self.portable);
    }
}

impl eframe::App for VdmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_tick.elapsed() >= std::time::Duration::from_millis(200) {
            self.last_tick = std::time::Instant::now();
            self.refresh();
        }
        let s = self.scheme;
        egui::TopBottomPanel::top("topbar")
            .frame(
                egui::Frame::default()
                    .fill(s.surface)
                    .inner_margin(egui::Margin::same(8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("VelocityDM")
                            .strong()
                            .color(s.primary)
                            .size(20.0),
                    );
                    ui.separator();
                    if ui.add(filled_button("Settings", &s)).clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let speed_txt = if self.total_speed > 0 {
                            format!("{}/s total", format_bytes(self.total_speed))
                        } else {
                            "idle".to_string()
                        };
                        ui.label(RichText::new(speed_txt).color(s.on_surface_variant));
                    });
                });
                ui.add_space(4.0);
                // add-download bar
                ui.horizontal(|ui| {
                    let hint = if self.add_url.is_empty() {
                        "Paste a URL, magnet: or path to .torrent"
                    } else {
                        ""
                    };
                    let edit = TextEdit::singleline(&mut self.add_url)
                        .hint_text(hint)
                        .desired_width(ui.available_width() - 460.0)
                        .margin(egui::vec2(10.0, 8.0));
                    ui.add(edit);
                    ui.label(RichText::new("quality").color(s.on_surface_variant));
                    ui.add(
                        TextEdit::singleline(&mut self.add_quality)
                            .desired_width(64.0)
                            .hint_text("best"),
                    );
                    ui.label(RichText::new("limit").color(s.on_surface_variant));
                    ui.add(
                        TextEdit::singleline(&mut self.add_limit)
                            .desired_width(70.0)
                            .hint_text("off"),
                    );
                    if ui.add(filled_button("Add", &s)).clicked()
                        || (ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && !self.add_url.is_empty())
                    {
                        self.submit_add();
                    }
                });
            });
        if self.show_settings {
            self.settings_panel(ctx);
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.rows.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new(
                            "No downloads yet.\nPaste a link above or use the browser extension.",
                        )
                        .color(s.on_surface_variant)
                        .size(16.0),
                    );
                });
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);
                    let rows = self.rows.clone();
                    for row in &rows {
                        self.task_row_ui(ui, row);
                    }
                });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        save_config(&self.engine.config(), self.portable);
        request_shutdown();
    }
}

impl VdmApp {
    fn submit_add(&mut self) {
        let url = self.add_url.trim().to_string();
        if url.is_empty() {
            return;
        }
        let limit = vdm_core::util::parse_bytes(&self.add_limit).unwrap_or(0);
        if url.starts_with("magnet:") {
            match vdm_core::bt::Magnet::parse(&url) {
                Ok(m) => {
                    let _ = self.engine.add_torrent(
                        vdm_core::bt::BtSpec::Magnet(m),
                        None,
                        vec![],
                        limit,
                    );
                    self.add_url.clear();
                }
                Err(e) => self.add_url = format!("! {}", e),
            }
            return;
        }
        if url.ends_with(".torrent") || std::path::Path::new(&url).exists() {
            let path = std::path::PathBuf::from(&url);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let _ = self.engine.add_torrent(
                        vdm_core::bt::BtSpec::TorrentBytes(bytes),
                        None,
                        vec![],
                        limit,
                    );
                    self.add_url.clear();
                }
                Err(e) => self.add_url = format!("! read: {}", e),
            }
            return;
        }
        let mut opts = HttpOptions {
            quality: self.add_quality.trim().to_string(),
            rate_limit_bps: limit,
            ..Default::default()
        };
        if !self.add_filename.trim().is_empty() {
            opts.filename = Some(self.add_filename.trim().to_string());
        }
        match self.engine.add_download(&url, opts) {
            Ok(_) => {
                self.add_url.clear();
                self.add_filename.clear();
            }
            Err(e) => self.add_url = format!("! {}", e),
        }
    }

    fn task_row_ui(&mut self, ui: &mut egui::Ui, row: &TaskRow) {
        let s = self.scheme;
        let is_selected = self.selected_task == Some(row.id);
        let frame = egui::Frame::default()
            .fill(if is_selected {
                s.primary_container
            } else {
                s.surface_container_low
            })
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .outer_margin(egui::Margin::symmetric(0.0, 2.0))
            .rounding(Rounding::same(12.0));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_width(ui.available_width());
                // MD3 state chip
                match row.is_state {
                    TaskState::Completed => chip(
                        ui,
                        "done",
                        s.on_tertiary_container,
                        s.tertiary_container,
                        Color32::TRANSPARENT,
                    ),
                    TaskState::Failed => chip(
                        ui,
                        &row.state,
                        s.on_error_container,
                        s.error_container,
                        Color32::TRANSPARENT,
                    ),
                    TaskState::Paused | TaskState::Queued => chip(
                        ui,
                        &row.state,
                        s.on_surface_variant,
                        Color32::TRANSPARENT,
                        s.outline,
                    ),
                    TaskState::Downloading => chip(
                        ui,
                        "active",
                        s.on_primary_container,
                        s.primary_container,
                        Color32::TRANSPARENT,
                    ),
                    TaskState::Verifying | TaskState::Probing => chip(
                        ui,
                        &row.state,
                        s.on_secondary_container,
                        s.secondary_container,
                        Color32::TRANSPARENT,
                    ),
                    TaskState::Seeding => chip(
                        ui,
                        "seed",
                        s.on_tertiary_container,
                        s.tertiary_container,
                        Color32::TRANSPARENT,
                    ),
                }
                ui.label(
                    RichText::new(format!("#{}", row.id))
                        .color(s.on_surface_variant)
                        .size(12.0),
                );
                ui.label(RichText::new(&row.name).color(s.on_surface).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // actions
                    let can_pause = matches!(
                        row.is_state,
                        TaskState::Downloading | TaskState::Probing | TaskState::Queued
                    );
                    let can_resume = matches!(row.is_state, TaskState::Paused | TaskState::Failed);
                    if can_pause && ui.add(tonal_button("Pause", &s)).clicked() {
                        let _ = self.engine.pause(row.id);
                    }
                    if can_resume && ui.add(tonal_button("Resume", &s)).clicked() {
                        let _ = self.engine.resume(row.id);
                    }
                    if ui.add(error_button("Remove", &s)).clicked() {
                        let _ = self.engine.remove(row.id, false);
                    }
                    // stats
                    let eta = if row.eta_secs > 0 && row.is_state == TaskState::Downloading {
                        format!("{}s", row.eta_secs)
                    } else {
                        "-".to_string()
                    };
                    let extra = if row.peers > 0 {
                        format!(" {} peers/{} seeds", row.peers, row.seeds)
                    } else {
                        String::new()
                    };
                    let speed = if row.speed > 0 {
                        format!("{}/s", format_bytes(row.speed))
                    } else {
                        "-".to_string()
                    };
                    ui.label(
                        RichText::new(format!(
                            "{} / {}  {}  {}{}",
                            format_bytes(row.done),
                            if row.size > 0 {
                                format_bytes(row.size)
                            } else {
                                "?".into()
                            },
                            speed,
                            eta,
                            extra
                        ))
                        .color(s.on_surface_variant)
                        .size(12.0),
                    );
                });
            });
            // progress indicator
            let frac = if row.size > 0 {
                (row.done as f32 / row.size as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let bar = egui::ProgressBar::new(frac)
                .desired_height(8.0)
                .fill(s.primary)
                .show_percentage();
            ui.add(bar);
            if let Some(err) = &row.error {
                ui.label(RichText::new(err).color(s.error).size(12.0));
            } else if !row.out_path.is_empty() && row.is_state == TaskState::Completed {
                ui.label(
                    RichText::new(&row.out_path)
                        .color(s.on_surface_variant)
                        .size(11.0),
                );
            }
            let resp = ui.interact(
                ui.min_rect(),
                egui::Id::new(("task", row.id)),
                egui::Sense::click(),
            );
            if resp.clicked() {
                self.selected_task = Some(row.id);
            }
        });
    }

    fn settings_panel(&mut self, ctx: &egui::Context) {
        let s = self.scheme;
        egui::TopBottomPanel::top("settings")
            .frame(
                egui::Frame::default()
                    .fill(s.surface_container_low)
                    .inner_margin(egui::Margin::same(14.0)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Settings")
                        .strong()
                        .color(s.primary)
                        .size(16.0),
                );
                ui.add_space(6.0);
                ui.columns(2, |cols| {
                    cols[0].horizontal(|ui| {
                        ui.label("Download folder");
                        let mut dir = self.settings.download_dir.to_string_lossy().to_string();
                        if ui
                            .add(TextEdit::singleline(&mut dir).desired_width(320.0))
                            .changed()
                        {
                            self.settings.download_dir = std::path::PathBuf::from(dir);
                        }
                    });
                    cols[1].horizontal(|ui| {
                        ui.label("Connections");
                        let mut conn = self.settings.default_connections;
                        if ui.add(Slider::new(&mut conn, 1..=32)).changed() {
                            self.settings.default_connections = conn;
                        }
                    });
                });
                ui.columns(2, |cols| {
                    cols[0].horizontal(|ui| {
                        ui.label("Global limit");
                        let mut limit = if self.settings.global_limit_bps > 0 {
                            format_bytes(self.settings.global_limit_bps)
                        } else {
                            String::new()
                        };
                        if ui
                            .add(
                                TextEdit::singleline(&mut limit)
                                    .desired_width(120.0)
                                    .hint_text("off"),
                            )
                            .changed()
                        {
                            self.settings.global_limit_bps =
                                vdm_core::util::parse_bytes(&limit).unwrap_or(0);
                        }
                    });
                    cols[1].horizontal(|ui| {
                        ui.label("Proxy");
                        ui.add(
                            TextEdit::singleline(&mut self.proxy_text)
                                .desired_width(280.0)
                                .hint_text("http://h:port or socks5://h:port"),
                        );
                    });
                });
                ui.columns(2, |cols| {
                    cols[0].horizontal(|ui| {
                        ui.label("Stop window");
                        ui.add(
                            TextEdit::singleline(&mut self.stop_start)
                                .desired_width(64.0)
                                .hint_text("22:00"),
                        );
                        ui.label("to");
                        ui.add(
                            TextEdit::singleline(&mut self.stop_end)
                                .desired_width(64.0)
                                .hint_text("06:00"),
                        );
                    });
                    cols[1].horizontal(|ui| {
                        let mut sd = self.settings.scheduler.shutdown_when_done;
                        if ui
                            .checkbox(&mut sd, "Shut down when queue drains")
                            .changed()
                        {
                            self.settings.scheduler.shutdown_when_done = sd;
                        }
                    });
                });
                ui.columns(2, |cols| {
                    cols[0].horizontal(|ui| {
                        ui.label("Max simultaneous");
                        let mut m = self.settings.scheduler.max_active;
                        if ui.add(Slider::new(&mut m, 1..=16)).changed() {
                            self.settings.scheduler.max_active = m;
                        }
                    });
                    cols[1].horizontal(|ui| {
                        if ui.add(filled_button("Apply", &s)).clicked() {
                            self.apply_settings();
                        }
                    });
                });
                // ---- Material You: appearance ----
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Material You")
                            .strong()
                            .color(s.primary)
                            .size(14.0),
                    );
                    ui.separator();
                    if ui
                        .add(tonal_button(
                            if self.prefs.dark {
                                "Dark mode"
                            } else {
                                "Light mode"
                            },
                            &s,
                        ))
                        .clicked()
                    {
                        let dark = !self.prefs.dark;
                        let seed = self.prefs.seed;
                        self.set_theme(ctx, seed, dark);
                    }
                    ui.separator();
                    ui.label(RichText::new("Dynamic color").color(s.on_surface_variant));
                    for (seed_hex, name) in theme::SEEDS {
                        let selected = self.prefs.seed == *seed_hex;
                        let fill = Color32::from_rgb(
                            (seed_hex >> 16) as u8,
                            (seed_hex >> 8) as u8,
                            *seed_hex as u8,
                        );
                        let mut btn = egui::Button::new(RichText::new(" ").size(14.0))
                            .fill(fill)
                            .min_size(Vec2::new(26.0, 26.0))
                            .rounding(Rounding::same(9999.0));
                        btn = if selected {
                            btn.stroke(Stroke::new(2.5_f32, s.on_surface))
                        } else {
                            btn.stroke(Stroke::new(1.0_f32, s.outline_variant))
                        };
                        if ui.add(btn).on_hover_text(*name).clicked() {
                            let (seed, dark) = (*seed_hex, self.prefs.dark);
                            self.set_theme(ctx, seed, dark);
                        }
                    }
                    ui.label(
                        RichText::new(theme::seed_name(self.prefs.seed))
                            .color(s.on_surface_variant)
                            .size(12.0),
                    );
                });
            });
    }

    fn apply_settings(&mut self) {
        let mut cfg = self.settings.clone();
        if self.proxy_text.trim().is_empty() || self.proxy_text.trim().eq_ignore_ascii_case("none")
        {
            cfg.proxy = None;
        } else if let Ok(p) = ProxyConfig::parse_url(self.proxy_text.trim()) {
            cfg.proxy = Some(p);
        }
        if vdm_core::engine::scheduler::parse_hhmm(&self.stop_start).is_some()
            && vdm_core::engine::scheduler::parse_hhmm(&self.stop_end).is_some()
        {
            cfg.scheduler.stop_window = Some((self.stop_start.clone(), self.stop_end.clone()));
        } else {
            cfg.scheduler.stop_window = None;
        }
        self.engine.set_config(cfg.clone());
        save_config(&cfg, self.portable);
        self.settings = cfg;
    }
}

fn proxy_display(p: &ProxyConfig) -> String {
    match p {
        ProxyConfig::Http { host, port, .. } => format!("http://{}:{}", host, port),
        ProxyConfig::Socks5 { host, port, .. } => format!("socks5://{}:{}", host, port),
    }
}
