//! VelocityDM Linux desktop frontend — GTK3.
//!
//! The "older-distro Linux" stack from the platform matrix (S12-lin2,
//! gate 0012): this binary embeds the shared Rust core in-process
//! (engine + JSON-RPC server, exactly like the egui GUI, the Slint GUI
//! and the daemon), renders Material You chrome from the shared
//! `vdm-md3` token engine through a GTK3 `GtkCssProvider`, and uses
//! native GTK widgets for platform-standard interaction. Talks to the
//! SAME config.json / ui.json as the other frontends. Preview surface:
//! ships in no release asset until its promotion gate.
//!
//! Security posture (spec rules 4-6): every attacker-controlled string
//! renders through `set_text` (this crate never calls `set_markup`),
//! and the stylesheet is generated only from `vdm-md3` token hex.

#![forbid(unsafe_code)]

mod config;
mod theme;

use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Button, CheckButton, CssProvider, Entry, HeaderBar,
    Label, ListBox, Orientation, PolicyType, ProgressBar, Revealer, RevealerTransitionType,
    ScrolledWindow, SpinButton,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use vdm_core::engine::events::TaskSnapshot;
use vdm_core::engine::{Engine, EngineConfig, HttpOptions};
use vdm_core::util::format_bytes;
use vdm_rpc::RpcServer;

const APP_ID: &str = "io.velocitydm.gtk3";
const REFRESH_MS: u64 = 200;

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

    let app = Application::builder().application_id(APP_ID).build();
    let engine_ui = engine.clone();
    app.connect_activate(move |app| build_ui(app, &engine_ui, cfg.clone(), prefs, portable));

    // gio's `run()` forwards std::env::args() to g_application_run, and
    // an unknown option (--portable) aborts startup — pass argv[0] only.
    let argv0 = std::env::args()
        .next()
        .unwrap_or_else(|| "velocitydm-gtk3".to_string());
    app.run_with_args(&[argv0]);

    engine.shutdown();
    server.stop();
    ExitCode::SUCCESS
}

struct TaskRow {
    name: Label,
    chip: Label,
    bar: ProgressBar,
    stats: Label,
    error: Label,
    notice: Label,
    pause: Button,
    resume: Button,
}

/// Apply the token stylesheet to the provider. The provider is
/// registered with the screen ONCE (in build_ui); theme changes only
/// reload its data, so no provider stacking occurs.
fn apply_css(provider: &CssProvider, seed: u32, dark: bool) {
    let css = theme::stylesheet(&vdm_md3::Scheme::from_seed_hex(seed, dark));
    if let Err(e) = provider.load_from_data(css.as_bytes()) {
        eprintln!("stylesheet: {e}");
    }
}

/// Push an EngineConfig into the settings widgets.
fn push_settings(widgets: &SettingsWidgets, cfg: &EngineConfig) {
    widgets.dir.set_text(&cfg.download_dir.to_string_lossy());
    widgets
        .connections
        .set_value(cfg.default_connections as f64);
    widgets
        .max_active
        .set_value(cfg.scheduler.max_active as f64);
    let limit = if cfg.global_limit_bps > 0 {
        format_bytes(cfg.global_limit_bps)
    } else {
        String::new()
    };
    widgets.limit.set_text(&limit);
    let proxy = cfg
        .proxy
        .as_ref()
        .map(theme::proxy_display)
        .unwrap_or_default();
    widgets.proxy.set_text(&proxy);
    let (start, end) = cfg.scheduler.stop_window.clone().unwrap_or_default();
    widgets.stop_start.set_text(&start);
    widgets.stop_end.set_text(&end);
    widgets
        .shutdown
        .set_active(cfg.scheduler.shutdown_when_done);
}

#[derive(Clone)]
struct SettingsWidgets {
    dir: Entry,
    connections: SpinButton,
    max_active: SpinButton,
    limit: Entry,
    proxy: Entry,
    stop_start: Entry,
    stop_end: Entry,
    shutdown: CheckButton,
}

/// In-place row update (no per-tick rebuild): widgets mutate, the chip
/// class swaps only when the state class changes.
fn update_row(row: &mut TaskRow, chip_class: &mut String, s: &TaskSnapshot) {
    row.name.set_text(&theme::display_text(&s.name));
    let (text, class) = theme::state_chip(s.state);
    if chip_class != class {
        let ctx = row.chip.style_context();
        if !chip_class.is_empty() {
            ctx.remove_class(chip_class);
        }
        ctx.add_class(class);
        *chip_class = class.to_string();
    }
    row.chip.set_text(text);
    row.bar
        .set_fraction(theme::progress_fraction(s.done, s.size));
    row.stats.set_text(&theme::stats_line(s));
    match &s.error {
        Some(e) if !e.is_empty() => {
            row.error.set_text(&theme::display_text(e));
            row.error.set_visible(true);
        }
        _ => row.error.set_visible(false),
    }
    match &s.notice {
        Some(n) if !n.is_empty() => {
            row.notice.set_text(&theme::display_text(n));
            row.notice.set_visible(true);
        }
        _ => row.notice.set_visible(false),
    }
    row.pause.set_visible(theme::can_pause(s.state));
    row.resume.set_visible(theme::can_resume(s.state));
}

/// Add-path routing — identical to the Slint frontend, plus the
/// filename override (the gap gtk3 closes over Win32/Slint/Swift).
fn spawn_from_entries(
    engine: &Engine,
    url: &str,
    filename: &str,
    quality: &str,
    limit: &str,
) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let limit = vdm_core::util::parse_bytes(limit.trim()).unwrap_or(0);
    let quality = quality.trim().to_string();
    let filename = filename.trim();
    let res = if url.starts_with("magnet:") {
        vdm_core::bt::Magnet::parse(url)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                engine
                    .add_torrent(vdm_core::bt::BtSpec::Magnet(m), None, vec![], limit)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
    } else if url.ends_with(".torrent") || std::path::Path::new(url).exists() {
        std::fs::read(url)
            .map_err(|e| format!("read: {e}"))
            .and_then(|bytes| {
                engine
                    .add_torrent(
                        vdm_core::bt::BtSpec::TorrentBytes(bytes),
                        None,
                        vec![],
                        limit,
                    )
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
    } else {
        let opts = HttpOptions {
            quality,
            rate_limit_bps: limit,
            filename: if filename.is_empty() {
                None
            } else {
                Some(filename.to_string())
            },
            ..Default::default()
        };
        engine
            .add_download(url, opts)
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    res.err()
}

fn build_ui(
    app: &Application,
    engine: &Arc<Engine>,
    cfg: EngineConfig,
    prefs: config::UiPrefs,
    portable: bool,
) {
    let win = ApplicationWindow::builder()
        .application(app)
        .title("VelocityDM")
        .default_width(880)
        .default_height(580)
        .build();

    // ---- Material You stylesheet (registered once) -------------------
    let provider = CssProvider::new();
    let screen = gtk::gdk::Screen::default();
    if let Some(screen) = &screen {
        gtk::StyleContext::add_provider_for_screen(
            screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    apply_css(&provider, prefs.seed, prefs.dark);

    // ---- header bar: speed banner + Material You controls ------------
    let header = HeaderBar::new();
    header.set_show_close_button(true);
    let speed_label = Label::new(Some("idle"));
    speed_label.style_context().add_class("dim");
    header.set_custom_title(Some(&speed_label));

    let seed_label = Label::new(Some(vdm_md3::seed_name(prefs.seed)));
    seed_label.style_context().add_class("dim");
    let dark_btn = Button::with_label("Light/Dark");
    let theme_btn = Button::with_label("Theme");
    let settings_btn = Button::with_label("Settings");
    header.pack_start(&settings_btn);
    header.pack_end(&seed_label);
    header.pack_end(&dark_btn);
    header.pack_end(&theme_btn);

    win.set_titlebar(Some(&header));

    // ---- root layout --------------------------------------------------
    let root = gtk::Box::new(Orientation::Vertical, 6);
    root.set_margin_top(8);
    root.set_margin_bottom(8);
    root.set_margin_start(10);
    root.set_margin_end(10);
    win.add(&root);

    // ---- add row -------------------------------------------------------
    let add_row = gtk::Box::new(Orientation::Horizontal, 6);
    let url_entry = Entry::new();
    url_entry.set_placeholder_text(Some("URL, magnet: or .torrent path"));
    url_entry.set_hexpand(true);
    let name_entry = Entry::new();
    name_entry.set_placeholder_text(Some("filename (optional)"));
    name_entry.set_size_request(150, -1);
    let quality_entry = Entry::new();
    quality_entry.set_text("best");
    quality_entry.set_placeholder_text(Some("quality"));
    quality_entry.set_size_request(90, -1);
    let limit_entry = Entry::new();
    limit_entry.set_placeholder_text(Some("limit (e.g. 2M)"));
    limit_entry.set_size_request(120, -1);
    let add_btn = Button::with_label("Add");
    add_row.pack_start(&url_entry, true, true, 0);
    add_row.pack_start(&name_entry, false, false, 0);
    add_row.pack_start(&quality_entry, false, false, 0);
    add_row.pack_start(&limit_entry, false, false, 0);
    add_row.pack_start(&add_btn, false, false, 0);
    root.pack_start(&add_row, false, false, 0);

    let add_error = Label::new(None);
    add_error.style_context().add_class("error-text");
    add_error.set_halign(Align::Start);
    add_error.set_line_wrap(true);
    add_error.set_visible(false);
    root.pack_start(&add_error, false, false, 0);

    // ---- task list ------------------------------------------------------
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .build();
    let list = ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    scroll.add(&list);
    root.pack_start(&scroll, true, true, 0);

    let empty = Label::new(Some("no tasks — add a download above"));
    empty.style_context().add_class("dim");
    empty.set_halign(Align::Center);
    empty.set_margin_top(18);
    empty.set_visible(false);
    root.pack_start(&empty, false, false, 0);

    // ---- settings panel --------------------------------------------------
    let settings_rev = Revealer::new();
    settings_rev.set_transition_type(RevealerTransitionType::SlideDown);
    settings_rev.set_reveal_child(false);
    let grid = gtk::Grid::new();
    grid.set_column_spacing(8);
    grid.set_row_spacing(6);
    grid.set_margin_top(6);
    grid.set_margin_bottom(6);

    let dir_entry = Entry::new();
    dir_entry.set_placeholder_text(Some("download dir"));
    dir_entry.set_hexpand(true);
    let conn_spin = SpinButton::with_range(1.0, 32.0, 1.0);
    let max_spin = SpinButton::with_range(1.0, 16.0, 1.0);
    let conn_lbl = Label::new(Some("connections"));
    conn_lbl.style_context().add_class("dim");
    let max_lbl = Label::new(Some("max active"));
    max_lbl.style_context().add_class("dim");
    let limit_settings = Entry::new();
    limit_settings.set_placeholder_text(Some("global speed limit (e.g. 5M, empty = none)"));
    let proxy_entry = Entry::new();
    proxy_entry.set_placeholder_text(Some(
        "proxy (http://host:port or socks5://host:port, 'none' to clear)",
    ));
    let stop_lbl = Label::new(Some("stop window"));
    stop_lbl.style_context().add_class("dim");
    let stop_row = gtk::Box::new(Orientation::Horizontal, 6);
    let stop_start_entry = Entry::new();
    stop_start_entry.set_placeholder_text(Some("start HH:MM"));
    let stop_end_entry = Entry::new();
    stop_end_entry.set_placeholder_text(Some("end HH:MM"));
    stop_row.pack_start(&stop_start_entry, true, true, 0);
    stop_row.pack_start(&stop_end_entry, true, true, 0);
    let shutdown_check = CheckButton::with_label("shut down when the queue drains");
    let apply_btn = Button::with_label("Apply");

    grid.attach(&dir_entry, 0, 0, 2, 1);
    grid.attach(&conn_lbl, 0, 1, 1, 1);
    grid.attach(&conn_spin, 1, 1, 1, 1);
    grid.attach(&max_lbl, 0, 2, 1, 1);
    grid.attach(&max_spin, 1, 2, 1, 1);
    grid.attach(&limit_settings, 0, 3, 2, 1);
    grid.attach(&proxy_entry, 0, 4, 2, 1);
    grid.attach(&stop_lbl, 0, 5, 1, 1);
    grid.attach(&stop_row, 1, 5, 1, 1);
    grid.attach(&shutdown_check, 0, 6, 2, 1);
    grid.attach(&apply_btn, 0, 7, 2, 1);

    let settings_widgets = SettingsWidgets {
        dir: dir_entry.clone(),
        connections: conn_spin.clone(),
        max_active: max_spin.clone(),
        limit: limit_settings.clone(),
        proxy: proxy_entry.clone(),
        stop_start: stop_start_entry.clone(),
        stop_end: stop_end_entry.clone(),
        shutdown: shutdown_check.clone(),
    };
    push_settings(&settings_widgets, &cfg);
    settings_rev.add(&grid);
    root.pack_start(&settings_rev, false, false, 0);

    // ---- add-path handlers (button + Enter key share the logic) -----
    let do_add: Rc<dyn Fn()> = Rc::new({
        let engine_a = engine.clone();
        let url_r = url_entry.clone();
        let name_r = name_entry.clone();
        let quality_r = quality_entry.clone();
        let limit_r = limit_entry.clone();
        let err_r = add_error.clone();
        move || match spawn_from_entries(
            &engine_a,
            &url_r.text(),
            &name_r.text(),
            &quality_r.text(),
            &limit_r.text(),
        ) {
            Some(e) => {
                err_r.set_text(&format!("add failed: {e}"));
                err_r.set_visible(true);
            }
            None => {
                err_r.set_visible(false);
                url_r.set_text("");
            }
        }
    });
    {
        let do_add = do_add.clone();
        add_btn.connect_clicked(move |_| do_add());
    }
    {
        let do_add = do_add.clone();
        url_entry.connect_activate(move |_| do_add());
    }

    // ---- per-task actions ------------------------------------------------
    let make_row: Rc<dyn Fn(u64) -> (gtk::Box, TaskRow)> = Rc::new({
        let engine_r = engine.clone();
        move |id: u64| {
            let vbox = gtk::Box::new(Orientation::Vertical, 3);
            vbox.set_margin_top(2);
            vbox.set_margin_bottom(2);
            vbox.set_margin_start(4);
            vbox.set_margin_end(4);
            let top = gtk::Box::new(Orientation::Horizontal, 6);
            let name = Label::new(None);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            name.set_halign(Align::Start);
            name.set_hexpand(true);
            let chip = Label::new(None);
            let pause = Button::with_label("Pause");
            let resume = Button::with_label("Resume");
            let remove = Button::with_label("Remove");
            top.pack_start(&name, true, true, 0);
            top.pack_start(&chip, false, false, 0);
            top.pack_start(&pause, false, false, 0);
            top.pack_start(&resume, false, false, 0);
            top.pack_start(&remove, false, false, 0);
            let bar = ProgressBar::new();
            bar.set_hexpand(true);
            let stats = Label::new(None);
            stats.style_context().add_class("dim");
            stats.set_halign(Align::Start);
            stats.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let error = Label::new(None);
            error.style_context().add_class("error-text");
            error.set_halign(Align::Start);
            error.set_line_wrap(true);
            error.set_visible(false);
            let notice = Label::new(None);
            notice.style_context().add_class("dim");
            notice.set_halign(Align::Start);
            notice.set_line_wrap(true);
            notice.set_visible(false);
            vbox.pack_start(&top, false, false, 0);
            vbox.pack_start(&bar, false, false, 0);
            vbox.pack_start(&stats, false, false, 0);
            vbox.pack_start(&error, false, false, 0);
            vbox.pack_start(&notice, false, false, 0);

            let e = engine_r.clone();
            pause.connect_clicked(move |_| {
                let _ = e.pause(id);
            });
            let e = engine_r.clone();
            resume.connect_clicked(move |_| {
                let _ = e.resume(id);
            });
            let e = engine_r.clone();
            remove.connect_clicked(move |_| {
                let _ = e.remove(id, false);
            });

            (
                vbox,
                TaskRow {
                    name,
                    chip,
                    bar,
                    stats,
                    error,
                    notice,
                    pause,
                    resume,
                },
            )
        }
    });

    type Rows = Rc<RefCell<HashMap<u64, (gtk::Box, TaskRow, String)>>>;
    let rows: Rows = Rc::new(RefCell::new(HashMap::new()));

    // ---- 200 ms refresh poll (slint parity; SSE is a later sub-stage)
    {
        let engine_t = engine.clone();
        let rows_t = rows.clone();
        let speed_r = speed_label.clone();
        let empty_r = empty.clone();
        let make_r = make_row.clone();
        let list_t = list.clone();
        let tick = glib::timeout_add_local(Duration::from_millis(REFRESH_MS), move || {
            let snaps = engine_t.list();
            let total: u64 = snaps.iter().map(|s| s.speed).sum();
            speed_r.set_text(&if total > 0 {
                format!("{}/s total", format_bytes(total))
            } else {
                "idle".to_string()
            });
            empty_r.set_visible(snaps.is_empty());
            let mut seen: HashSet<u64> = HashSet::new();
            {
                let mut map = rows_t.borrow_mut();
                for (idx, s) in snaps.iter().enumerate() {
                    seen.insert(s.id);
                    let entry = map.entry(s.id).or_insert_with(|| {
                        let (w, row) = make_r(s.id);
                        list_t.insert(&w, idx as i32);
                        (w, row, String::new())
                    });
                    update_row(&mut entry.1, &mut entry.2, s);
                }
            }
            // Tasks removed engine-side: drop their rows (widget destroy
            // detaches them from the ListBox).
            let stale: Vec<u64> = rows_t
                .borrow()
                .keys()
                .filter(|id| !seen.contains(id))
                .copied()
                .collect();
            for id in stale {
                if let Some((w, _, _)) = rows_t.borrow_mut().remove(&id) {
                    // gtk3-rs 0.18 has no WidgetExt::destroy — unparent the
                    // ListBoxRow wrapper (the inserted box's parent); the
                    // container-owned row is freed on removal.
                    if let Some(row) = w.parent() {
                        list_t.remove(&row);
                    }
                }
            }
            glib::ControlFlow::Continue
        });
        // The poll runs until process exit (the Slint frontend keeps its
        // timer alive for the whole run the same way).
        std::mem::forget(tick);
    }

    // ---- settings toggle + apply --------------------------------------
    {
        let rev = settings_rev.clone();
        settings_btn.connect_clicked(move |_| rev.set_reveal_child(!rev.reveals_child()));
    }
    {
        let engine_s = engine.clone();
        let portable_s = portable;
        let w = settings_widgets.clone();
        apply_btn.connect_clicked(move |_| {
            // Start from the live config so fields this panel does not
            // expose (user_agent, scheduler details) survive Apply.
            let mut new_cfg = engine_s.config();
            theme::apply_settings_fields(
                &mut new_cfg,
                &w.dir.text(),
                w.connections.value() as i32,
                w.max_active.value() as i32,
                &w.limit.text(),
                &w.proxy.text(),
                &w.stop_start.text(),
                &w.stop_end.text(),
                w.shutdown.is_active(),
            );
            engine_s.set_config(new_cfg.clone());
            config::save_config(&new_cfg, portable_s);
            push_settings(&w, &new_cfg);
        });
    }

    // ---- Material You controls ------------------------------------------
    {
        let portable_t = portable;
        let provider_r = provider.clone();
        let seed_r = seed_label.clone();
        theme_btn.connect_clicked(move |_| {
            let mut p = config::load_prefs(portable_t);
            let idx = vdm_md3::SEEDS
                .iter()
                .position(|(s, _)| *s == p.seed)
                .unwrap_or(0);
            p.seed = vdm_md3::SEEDS[(idx + 1) % vdm_md3::SEEDS.len()].0;
            config::save_prefs(&p, portable_t);
            apply_css(&provider_r, p.seed, p.dark);
            seed_r.set_text(vdm_md3::seed_name(p.seed));
        });
    }
    {
        let portable_t = portable;
        let provider_r = provider.clone();
        dark_btn.connect_clicked(move |_| {
            let mut p = config::load_prefs(portable_t);
            p.dark = !p.dark;
            config::save_prefs(&p, portable_t);
            apply_css(&provider_r, p.seed, p.dark);
        });
    }

    // ---- exit path: last window closed -> quit -> shutdown in main --
    {
        let app_q = app.clone();
        win.connect_destroy(move |_| app_q.quit());
    }

    win.show_all();
    // show_all resurrects everything — re-hide the labels that must
    // start hidden (the poll's first tick sets the honest empty state;
    // per-row labels are created by the poll, after this point).
    add_error.set_visible(false);
    empty.set_visible(false);
}
