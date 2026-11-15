//! VelocityDM Windows frontend — Raw Win32 + Direct2D.
//!
//! The "Windows 10+" stack from the platform matrix: a hand-rolled Win32
//! message loop and window (no framework), GPU-accelerated MD3 vector
//! chrome via Direct2D (rounded cards, chips, tonal buttons, progress
//! bars — geometry + brushes, no bitmaps), DirectWrite text, native
//! EDIT/BUTTON children for input (platform-standard interaction is the
//! OS-specific layer), and the shared Rust core in-process (engine +
//! JSON-RPC server) exactly like the daemon and the other frontends.
//! Colors come from the shared `vdm-md3` token engine.
//!
//! Windows 7 ships the headless payload by design; this frontend
//! targets Windows 10+ (MSVC).

#![allow(non_snake_case)]

mod config;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use vdm_core::engine::events::TaskState;
use vdm_core::engine::{Engine, EngineConfig, HttpOptions};
use vdm_core::proxy::ProxyConfig;
use vdm_core::util::format_bytes;
use vdm_rpc::RpcServer;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_REGULAR, DWRITE_MEASURING_MODE_NATURAL,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WC_BUTTON;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetTimer, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CW_USEDEFAULT, MSG,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND,
    WM_DESTROY, WM_LBUTTONDOWN, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSW, WNDPROC, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};

// ---------------------------------------------------------------------------
// Layout constants (96-dpi pixels; scaled by GetDpiForWindow)
// ---------------------------------------------------------------------------
const PAD: f32 = 16.0;
const HEADER_H: f32 = 56.0;
const ADDBAR_H: f32 = 48.0;
const SETTINGS_H: f32 = 118.0;
const CARD_H: f32 = 96.0;
const CARD_GAP: f32 = 8.0;
const CARD_PAD: f32 = 12.0;
const BTN_W: f32 = 78.0;
const BTN_H: f32 = 30.0;
const RADIUS: f32 = 12.0;
const REFRESH_MILLIS: u32 = 200;

// control IDs
const IDC_EDIT_URL: usize = 101;
const IDC_EDIT_QUALITY: usize = 102;
const IDC_EDIT_LIMIT: usize = 103;
const IDC_BTN_ADD: usize = 104;
const IDC_BTN_SETTINGS: usize = 105;
const IDC_BTN_SEED: usize = 106;
const IDC_BTN_DARK: usize = 107;
const IDC_BTN_APPLY: usize = 108;
const IDC_CHK_SHUTDOWN: usize = 109;
const IDC_EDIT_DIR: usize = 110;
const IDC_EDIT_CONN: usize = 111;
const IDC_EDIT_MAX_ACTIVE: usize = 112;
const IDC_EDIT_GLIMIT: usize = 113;
const IDC_EDIT_PROXY: usize = 114;
const IDC_EDIT_STOP_START: usize = 115;
const IDC_EDIT_STOP_END: usize = 116;

// ---------------------------------------------------------------------------
// Direct2D renderer state
// ---------------------------------------------------------------------------

fn col(hex: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

fn rectf(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}

struct D2d {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    rt: Option<ID2D1HwndRenderTarget>,
}

impl D2d {
    fn new() -> windows::core::Result<D2d> {
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let dwrite: IDWriteFactory =
            unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED)? };
        Ok(D2d {
            factory,
            dwrite,
            rt: None,
        })
    }

    fn ensure_target(&mut self, hwnd: HWND, w: u32, h: u32) -> windows::core::Result<()> {
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 0.0,
                dpiY: 0.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hwnd_props =
                windows::Win32::Graphics::Direct2D::D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd,
                    pixelSize: D2D_SIZE_U {
                        width: w.max(1),
                        height: h.max(1),
                    },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                };
            let rt: ID2D1HwndRenderTarget =
                unsafe { self.factory.CreateHwndRenderTarget(&props, &hwnd_props)? };
            self.rt = Some(rt);
        }
        Ok(())
    }

    fn reset_target(&mut self) {
        self.rt = None;
    }

    fn brush(&self, c: D2D1_COLOR_F) -> windows::core::Result<ID2D1SolidColorBrush> {
        let rt = self
            .rt
            .as_ref()
            .ok_or_else(|| windows::core::Error::from(windows::core::HRESULT(-1)))?;
        unsafe { rt.CreateSolidColorBrush(&c, None) }
    }

    fn text_format(&self, size: f32, bold: bool) -> windows::core::Result<IDWriteTextFormat> {
        unsafe {
            self.dwrite.CreateTextFormat(
                w!("Segoe UI"),
                None,
                if bold {
                    DWRITE_FONT_WEIGHT_BOLD
                } else {
                    DWRITE_FONT_WEIGHT_REGULAR
                },
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )
        }
    }

    fn text(
        &self,
        rt: &ID2D1HwndRenderTarget,
        s: &str,
        tf: &IDWriteTextFormat,
        where_: D2D_RECT_F,
        brush: &ID2D1SolidColorBrush,
    ) {
        let u16s: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            rt.DrawText(
                &u16s,
                tf,
                &where_,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    fn rounded(
        &self,
        rt: &ID2D1HwndRenderTarget,
        where_: D2D_RECT_F,
        radius: f32,
        brush: &ID2D1SolidColorBrush,
    ) {
        let rr = D2D1_ROUNDED_RECT {
            rect: where_,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { rt.FillRoundedRectangle(&rr, brush) };
    }

    fn outlined(
        &self,
        rt: &ID2D1HwndRenderTarget,
        where_: D2D_RECT_F,
        radius: f32,
        brush: &ID2D1SolidColorBrush,
    ) {
        let rr = D2D1_ROUNDED_RECT {
            rect: where_,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe {
            rt.DrawRoundedRectangle(&rr, brush, 1.0, None);
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TaskRow {
    id: u64,
    state: TaskState,
    state_text: String,
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

struct App {
    engine: Arc<Engine>,
    scheme: vdm_md3::Scheme,
    prefs: config::UiPrefs,
    rows: Vec<TaskRow>,
    total_speed: u64,
    show_settings: bool,
    portable: bool,
    dpi: f32,
    edit_url: HWND,
    edit_quality: HWND,
    edit_limit: HWND,
    edit_dir: HWND,
    edit_conn: HWND,
    edit_max_active: HWND,
    edit_glimit: HWND,
    edit_proxy: HWND,
    edit_stop_start: HWND,
    edit_stop_end: HWND,
    chk_shutdown: HWND,
    btn_add: HWND,
    btn_settings: HWND,
    btn_seed: HWND,
    btn_dark: HWND,
    btn_apply: HWND,
    zones: Vec<Zone>,
}

#[derive(Clone, Copy)]
enum Zone {
    Pause { r: D2D_RECT_F, id: u64 },
    Resume { r: D2D_RECT_F, id: u64 },
    Remove { r: D2D_RECT_F, id: u64 },
}

struct WndData {
    app: App,
    d2d: D2d,
}

thread_local! {
    static WND: RefCell<Option<Rc<RefCell<WndData>>>> = const { RefCell::new(None) };
}

fn with_data<R>(f: impl FnOnce(&mut WndData) -> R) -> Option<R> {
    WND.with(|slot| {
        let guard = slot.borrow();
        let data = guard.as_ref()?;
        let mut inner = data.borrow_mut();
        Some(f(&mut inner))
    })
}

fn state_text(state: TaskState) -> &'static str {
    match state {
        TaskState::Completed => "done",
        TaskState::Failed => "failed",
        TaskState::Paused => "paused",
        TaskState::Queued => "queued",
        TaskState::Downloading => "active",
        TaskState::Verifying => "verify",
        TaskState::Probing => "probe",
        TaskState::Seeding => "seed",
    }
}

fn proxy_display(p: &ProxyConfig) -> String {
    match p {
        ProxyConfig::Http { host, port, .. } => format!("http://{host}:{port}"),
        ProxyConfig::Socks5 { host, port, .. } => format!("socks5://{host}:{port}"),
    }
}

impl App {
    fn refresh(&mut self) {
        let snaps = self.engine.list();
        self.total_speed = snaps.iter().map(|s| s.speed).sum();
        self.rows = snaps
            .into_iter()
            .map(|s| TaskRow {
                id: s.id,
                state: s.state,
                state_text: state_text(s.state).to_string(),
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

    fn retheme(&mut self) {
        self.scheme = vdm_md3::Scheme::from_seed_hex(self.prefs.seed, self.prefs.dark);
    }
}

// ---------------------------------------------------------------------------
// Native child controls
// ---------------------------------------------------------------------------

fn instance() -> windows::core::Result<windows::Win32::Foundation::HMODULE> {
    unsafe { GetModuleHandleW(None) }
}

fn make_edit(
    parent: HWND,
    x: i32,
    y: i32,
    wpx: i32,
    hpx: i32,
    id: usize,
) -> windows::core::Result<HWND> {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            x,
            y,
            wpx,
            hpx,
            Some(parent),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(id as _)),
            None,
            None,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn make_btn(
    parent: HWND,
    x: i32,
    y: i32,
    wpx: i32,
    hpx: i32,
    id: usize,
    label: &str,
    checkbox: bool,
) -> windows::core::Result<HWND> {
    let style = if checkbox {
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32)
    } else {
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32)
    };
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            WC_BUTTON,
            &HSTRING::from(label),
            style,
            x,
            y,
            wpx,
            hpx,
            Some(parent),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(id as _)),
            None,
            None,
        )
    }
}

fn set_text(hwnd: HWND, s: &str) {
    unsafe {
        let _ = SetWindowTextW(hwnd, &HSTRING::from(s));
    }
}

fn get_text(hwnd: HWND) -> String {
    unsafe {
        let len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(hwnd) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn place(hwnd: HWND, x: i32, y: i32, wpx: i32, hpx: i32) {
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, wpx, hpx, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn show_ctrl(hwnd: HWND, visible: bool) {
    unsafe {
        let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
    }
}

fn checked(hwnd: HWND) -> bool {
    unsafe {
        let st = SendMessageW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
            None,
            None,
        );
        st.0 != 0
    }
}

fn set_checked(hwnd: HWND, on: bool) {
    unsafe {
        let _ = SendMessageW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::BM_SETCHECK,
            Some(WPARAM(if on { 1 } else { 0 })),
            None,
        );
    }
}

fn client_rect(hwnd: HWND) -> windows::Win32::Foundation::RECT {
    let mut rc = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    rc
}

// ---------------------------------------------------------------------------
// Layout + settings plumbing
// ---------------------------------------------------------------------------

fn layout_children(app: &App, rc: &windows::Win32::Foundation::RECT) {
    let s = |v: f32| (v * app.dpi / 96.0) as i32;
    let width = rc.right - rc.left;
    let y0 = HEADER_H + if app.show_settings { SETTINGS_H } else { 0.0 };
    let url_w = width - s(60.0 + 64.0 + 8.0 + 64.0 + 8.0 + 72.0 + 8.0 + PAD);
    place(app.edit_url, s(PAD), s(y0 + 10.0), url_w, s(24.0));
    place(
        app.edit_quality,
        s(PAD) + url_w + s(8.0),
        s(y0 + 10.0),
        s(64.0),
        s(24.0),
    );
    place(
        app.edit_limit,
        s(PAD) + url_w + s(80.0),
        s(y0 + 10.0),
        s(64.0),
        s(24.0),
    );
    place(
        app.btn_add,
        s(PAD) + url_w + s(152.0),
        s(y0 + 8.0),
        s(72.0),
        s(28.0),
    );

    // header controls
    place(app.btn_settings, s(240.0), s(14.0), s(84.0), s(28.0));
    place(app.btn_seed, s(336.0), s(14.0), s(120.0), s(28.0));
    place(app.btn_dark, s(466.0), s(14.0), s(64.0), s(28.0));

    // settings panel rows
    let sy = HEADER_H;
    place(app.edit_dir, s(PAD + 8.0), s(sy + 36.0), s(190.0), s(24.0));
    place(
        app.edit_conn,
        s(PAD + 210.0),
        s(sy + 36.0),
        s(52.0),
        s(24.0),
    );
    place(
        app.edit_max_active,
        s(PAD + 272.0),
        s(sy + 36.0),
        s(52.0),
        s(24.0),
    );
    place(
        app.edit_glimit,
        s(PAD + 334.0),
        s(sy + 36.0),
        s(80.0),
        s(24.0),
    );
    place(
        app.edit_proxy,
        s(PAD + 424.0),
        s(sy + 36.0),
        s(200.0),
        s(24.0),
    );
    place(
        app.edit_stop_start,
        s(PAD + 8.0),
        s(sy + 76.0),
        s(56.0),
        s(24.0),
    );
    place(
        app.edit_stop_end,
        s(PAD + 74.0),
        s(sy + 76.0),
        s(56.0),
        s(24.0),
    );
    place(
        app.chk_shutdown,
        s(PAD + 144.0),
        s(sy + 76.0),
        s(190.0),
        s(24.0),
    );
    place(
        app.btn_apply,
        s(PAD + 344.0),
        s(sy + 74.0),
        s(80.0),
        s(28.0),
    );
}

fn push_settings(app: &App, cfg: &EngineConfig) {
    set_text(app.edit_dir, &cfg.download_dir.to_string_lossy());
    set_text(app.edit_conn, &cfg.default_connections.to_string());
    set_text(app.edit_max_active, &cfg.scheduler.max_active.to_string());
    let limit = if cfg.global_limit_bps > 0 {
        format_bytes(cfg.global_limit_bps).to_string()
    } else {
        String::new()
    };
    set_text(app.edit_glimit, &limit);
    let proxy = cfg.proxy.as_ref().map(proxy_display).unwrap_or_default();
    set_text(app.edit_proxy, &proxy);
    let (start, end) = cfg
        .scheduler
        .stop_window
        .as_ref()
        .map(|w| (w.0.clone(), w.1.clone()))
        .unwrap_or_default();
    set_text(app.edit_stop_start, &start);
    set_text(app.edit_stop_end, &end);
    set_checked(app.chk_shutdown, cfg.scheduler.shutdown_when_done);
}

fn sync_settings_visibility(app: &App) {
    for h in [
        app.edit_dir,
        app.edit_conn,
        app.edit_max_active,
        app.edit_glimit,
        app.edit_proxy,
        app.edit_stop_start,
        app.edit_stop_end,
        app.chk_shutdown,
        app.btn_apply,
    ] {
        show_ctrl(h, app.show_settings);
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

fn add_download(app: &mut App) {
    let url = get_text(app.edit_url);
    let url = url.trim().to_string();
    if url.is_empty() {
        return;
    }
    let limit = vdm_core::util::parse_bytes(&get_text(app.edit_limit)).unwrap_or(0);
    let quality = get_text(app.edit_quality);
    let mut err = None;
    if url.starts_with("magnet:") {
        match vdm_core::bt::Magnet::parse(&url) {
            Ok(m) => {
                let _ =
                    app.engine
                        .add_torrent(vdm_core::bt::BtSpec::Magnet(m), None, vec![], limit);
            }
            Err(e) => err = Some(format!("! {e}")),
        }
    } else if url.ends_with(".torrent") || std::path::Path::new(&url).exists() {
        match std::fs::read(&url) {
            Ok(bytes) => {
                let _ = app.engine.add_torrent(
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
        if let Err(e) = app.engine.add_download(&url, opts) {
            err = Some(format!("! {e}"));
        }
    }
    match err {
        Some(e) => set_text(app.edit_url, &e),
        None => set_text(app.edit_url, ""),
    }
}

fn apply_settings(app: &mut App) {
    let mut cfg = app.engine.config();
    let dir = get_text(app.edit_dir);
    let dir = dir.trim().to_string();
    if !dir.is_empty() {
        cfg.download_dir = std::path::PathBuf::from(dir);
    }
    if let Ok(c) = get_text(app.edit_conn).trim().parse::<usize>() {
        cfg.default_connections = c.clamp(1, 32);
    }
    if let Ok(m) = get_text(app.edit_max_active).trim().parse::<usize>() {
        cfg.scheduler.max_active = m.clamp(1, 16);
    }
    cfg.global_limit_bps = vdm_core::util::parse_bytes(&get_text(app.edit_glimit)).unwrap_or(0);
    let proxy = get_text(app.edit_proxy);
    let proxy = proxy.trim().to_string();
    if proxy.is_empty() || proxy.eq_ignore_ascii_case("none") {
        cfg.proxy = None;
    } else if let Ok(p) = ProxyConfig::parse_url(&proxy) {
        cfg.proxy = Some(p);
    }
    let start = get_text(app.edit_stop_start);
    let end = get_text(app.edit_stop_end);
    if vdm_core::engine::scheduler::parse_hhmm(&start).is_some()
        && vdm_core::engine::scheduler::parse_hhmm(&end).is_some()
    {
        cfg.scheduler.stop_window = Some((start, end));
    } else {
        cfg.scheduler.stop_window = None;
    }
    cfg.scheduler.shutdown_when_done = checked(app.chk_shutdown);
    app.engine.set_config(cfg.clone());
    config::save_config(&cfg, app.portable);
}

// ---------------------------------------------------------------------------
// Rendering — Material You chrome (Direct2D vector drawing)
// ---------------------------------------------------------------------------

fn paint(data: &mut WndData, hwnd: HWND) {
    let rc = client_rect(hwnd);
    let w = (rc.right - rc.left).max(1) as u32;
    let h = (rc.bottom - rc.top).max(1) as u32;
    if data.d2d.ensure_target(hwnd, w, h).is_err() {
        return;
    }
    let rt = match data.d2d.rt.clone() {
        Some(rt) => rt,
        None => return,
    };
    let scheme = data.app.scheme;
    unsafe {
        rt.BeginDraw();
        rt.Clear(Some(&col(scheme.surface)));
    }

    let brushes = match make_brushes(&data.d2d, &scheme) {
        Some(b) => b,
        None => {
            data.d2d.reset_target();
            return;
        }
    };
    data.app.zones.clear();
    draw_header(data, &rt, &rc, &brushes);
    if data.app.show_settings {
        let k = data.app.dpi / 96.0;
        draw_settings_back(&data.d2d, &rt, &rc, &brushes, k);
    }
    draw_cards(data, &rt, &rc, &brushes);

    let hr = unsafe { rt.EndDraw(None, None) };
    if hr.is_err() {
        data.d2d.reset_target();
    }
}

struct Brushes {
    primary: ID2D1SolidColorBrush,
    sec_container: ID2D1SolidColorBrush,
    on_sec_container: ID2D1SolidColorBrush,
    card: ID2D1SolidColorBrush,
    card_low: ID2D1SolidColorBrush,
    track: ID2D1SolidColorBrush,
    on_surface: ID2D1SolidColorBrush,
    on_surface_variant: ID2D1SolidColorBrush,
    outline: ID2D1SolidColorBrush,
    error: ID2D1SolidColorBrush,
}

fn make_brushes(d2d: &D2d, s: &vdm_md3::Scheme) -> Option<Brushes> {
    Some(Brushes {
        primary: d2d.brush(col(s.primary)).ok()?,
        sec_container: d2d.brush(col(s.secondary_container)).ok()?,
        on_sec_container: d2d.brush(col(s.on_secondary_container)).ok()?,
        card: d2d.brush(col(s.surface_container)).ok()?,
        card_low: d2d.brush(col(s.surface_container_low)).ok()?,
        track: d2d.brush(col(s.surface_container_highest)).ok()?,
        on_surface: d2d.brush(col(s.on_surface)).ok()?,
        on_surface_variant: d2d.brush(col(s.on_surface_variant)).ok()?,
        outline: d2d.brush(col(s.outline_variant)).ok()?,
        error: d2d.brush(col(s.error)).ok()?,
    })
}

fn draw_header(
    data: &WndData,
    rt: &ID2D1HwndRenderTarget,
    rc: &windows::Win32::Foundation::RECT,
    b: &Brushes,
) {
    let k = data.app.dpi / 96.0;
    let px = |v: f32| v * k;
    let Ok(title) = data.d2d.text_format(20.0 * k, true) else {
        return;
    };
    let Ok(dim) = data.d2d.text_format(12.0 * k, false) else {
        return;
    };
    data.d2d.text(
        rt,
        "VelocityDM",
        &title,
        rectf(px(PAD), px(16.0), px(240.0), px(HEADER_H)),
        &b.primary,
    );
    let speed_txt = if data.app.total_speed > 0 {
        format!("{}/s total", format_bytes(data.app.total_speed))
    } else {
        "idle".to_string()
    };
    let wpx = (rc.right - rc.left) as f32;
    data.d2d.text(
        rt,
        &speed_txt,
        &dim,
        rectf(wpx - px(200.0), px(20.0), wpx - px(PAD), px(HEADER_H)),
        &b.on_surface_variant,
    );
}

fn draw_settings_back(
    d2d: &D2d,
    rt: &ID2D1HwndRenderTarget,
    rc: &windows::Win32::Foundation::RECT,
    b: &Brushes,
    k: f32,
) {
    let px = |v: f32| v * k;
    let wpx = (rc.right - rc.left) as f32;
    let back = rectf(
        px(PAD),
        px(HEADER_H + 6.0),
        wpx - px(PAD),
        px(HEADER_H + SETTINGS_H - 6.0),
    );
    d2d.rounded(rt, back, px(RADIUS), &b.card_low);
    d2d.outlined(rt, back, px(RADIUS), &b.outline);
    if let Ok(tf) = d2d.text_format(16.0 * k, true) {
        d2d.text(
            rt,
            "Settings",
            &tf,
            rectf(
                px(PAD + 10.0),
                px(HEADER_H + 10.0),
                px(200.0),
                px(HEADER_H + 34.0),
            ),
            &b.primary,
        );
    }
    if let Ok(tf) = d2d.text_format(11.0 * k, false) {
        d2d.text(
            rt,
            "Folder / connections / max / global limit / proxy",
            &tf,
            rectf(
                px(PAD + 12.0),
                px(HEADER_H + 16.0),
                px(PAD + 640.0),
                px(HEADER_H + 30.0),
            ),
            &b.on_surface_variant,
        );
        d2d.text(
            rt,
            "Stop window",
            &tf,
            rectf(
                px(PAD + 10.0),
                px(HEADER_H + 58.0),
                px(PAD + 90.0),
                px(HEADER_H + 72.0),
            ),
            &b.on_surface_variant,
        );
    }
}

fn draw_cards(
    data: &mut WndData,
    rt: &ID2D1HwndRenderTarget,
    rc: &windows::Win32::Foundation::RECT,
    b: &Brushes,
) {
    let scheme = data.app.scheme;
    let k = data.app.dpi / 96.0;
    let px = |v: f32| v * k;
    let wpx = (rc.right - rc.left) as f32;
    let y0 = px(HEADER_H
        + if data.app.show_settings {
            SETTINGS_H
        } else {
            0.0
        }
        + ADDBAR_H
        + PAD);
    let Ok(name_tf) = data.d2d.text_format(14.0 * k, true) else {
        return;
    };
    let Ok(stat_tf) = data.d2d.text_format(12.0 * k, false) else {
        return;
    };
    let Ok(chip_tf) = data.d2d.text_format(11.0 * k, true) else {
        return;
    };
    let Ok(empty_tf) = data.d2d.text_format(16.0 * k, false) else {
        return;
    };

    if data.app.rows.is_empty() {
        data.d2d.text(
            rt,
            "No downloads yet. Paste a link above or use the browser extension.",
            &empty_tf,
            rectf(px(PAD), y0 + px(40.0), wpx - px(PAD), y0 + px(80.0)),
            &b.on_surface_variant,
        );
        return;
    }

    let mut y = y0;
    for row in data.app.rows.clone() {
        let card = rectf(px(PAD), y, wpx - px(PAD), y + px(CARD_H));
        data.d2d.rounded(rt, card, px(RADIUS), &b.card);
        data.d2d.outlined(rt, card, px(RADIUS), &b.outline);

        // state chip
        let chip_color = match row.state {
            TaskState::Completed => scheme.tertiary,
            TaskState::Failed => scheme.error,
            TaskState::Paused | TaskState::Queued => scheme.on_surface_variant,
            TaskState::Downloading | TaskState::Seeding => scheme.primary,
            TaskState::Verifying | TaskState::Probing => scheme.secondary_container,
        };
        let Ok(chip_br) = data.d2d.brush(col(chip_color)) else {
            continue;
        };
        let chip = rectf(
            px(PAD + CARD_PAD),
            y + px(CARD_PAD),
            px(PAD + CARD_PAD + 52.0),
            y + px(CARD_PAD + 22.0),
        );
        data.d2d.rounded(rt, chip, px(8.0), &chip_br);
        data.d2d.text(
            rt,
            &row.state_text,
            &chip_tf,
            rectf(
                px(PAD + CARD_PAD + 6.0),
                y + px(CARD_PAD + 2.0),
                px(PAD + CARD_PAD + 50.0),
                y + px(CARD_PAD + 20.0),
            ),
            &b.on_surface,
        );

        // title line
        data.d2d.text(
            rt,
            &format!("#{}  {}", row.id, row.name),
            &name_tf,
            rectf(
                px(PAD + CARD_PAD + 60.0),
                y + px(CARD_PAD - 2.0),
                wpx - px(PAD + CARD_PAD + 3.0 * BTN_W + 40.0),
                y + px(CARD_PAD + 24.0),
            ),
            &b.on_surface,
        );

        // stats line
        let eta = if row.eta_secs > 0 && row.state == TaskState::Downloading {
            format!("{}s", row.eta_secs)
        } else {
            "-".to_string()
        };
        let extra = if row.peers > 0 {
            format!(" {}p/{}s", row.peers, row.seeds)
        } else {
            String::new()
        };
        let speed = if row.speed > 0 {
            format!("{}/s", format_bytes(row.speed))
        } else {
            "-".to_string()
        };
        let stats = format!(
            "{} / {}  {}  {}{}",
            format_bytes(row.done),
            if row.size > 0 {
                format_bytes(row.size)
            } else {
                "?".to_string()
            },
            speed,
            eta,
            extra
        );
        data.d2d.text(
            rt,
            &stats,
            &stat_tf,
            rectf(
                px(PAD + CARD_PAD),
                y + px(CARD_PAD + 26.0),
                wpx - px(PAD + CARD_PAD + 3.0 * BTN_W + 56.0),
                y + px(CARD_H - 26.0),
            ),
            &b.on_surface_variant,
        );

        // drawn MD3 buttons (hit-tested): Remove / Resume / Pause
        let can_pause = matches!(
            row.state,
            TaskState::Downloading | TaskState::Probing | TaskState::Queued
        );
        let can_resume = matches!(row.state, TaskState::Paused | TaskState::Failed);
        let by = y + px(CARD_H - BTN_H - CARD_PAD + 4.0);
        let mut bx = wpx - px(PAD + CARD_PAD + BTN_W);
        let remove = rectf(bx, by, bx + px(BTN_W), by + px(BTN_H));
        data.d2d.rounded(rt, remove, px(BTN_H / 2.0), &b.card_low);
        data.d2d.outlined(rt, remove, px(BTN_H / 2.0), &b.outline);
        data.d2d.text(
            rt,
            "Remove",
            &stat_tf,
            rectf(
                bx + px(14.0),
                by + px(6.0),
                bx + px(BTN_W - 8.0),
                by + px(BTN_H),
            ),
            &b.on_surface_variant,
        );
        data.app.zones.push(Zone::Remove {
            r: remove,
            id: row.id,
        });
        if can_resume {
            bx -= px(BTN_W + 8.0);
            let resume = rectf(bx, by, bx + px(BTN_W), by + px(BTN_H));
            data.d2d
                .rounded(rt, resume, px(BTN_H / 2.0), &b.sec_container);
            data.d2d.text(
                rt,
                "Resume",
                &stat_tf,
                rectf(
                    bx + px(14.0),
                    by + px(6.0),
                    bx + px(BTN_W - 8.0),
                    by + px(BTN_H),
                ),
                &b.on_sec_container,
            );
            data.app.zones.push(Zone::Resume {
                r: resume,
                id: row.id,
            });
        }
        if can_pause {
            bx -= px(BTN_W + 8.0);
            let pause = rectf(bx, by, bx + px(BTN_W), by + px(BTN_H));
            data.d2d
                .rounded(rt, pause, px(BTN_H / 2.0), &b.sec_container);
            data.d2d.text(
                rt,
                "Pause",
                &stat_tf,
                rectf(
                    bx + px(14.0),
                    by + px(6.0),
                    bx + px(BTN_W - 8.0),
                    by + px(BTN_H),
                ),
                &b.on_sec_container,
            );
            data.app.zones.push(Zone::Pause {
                r: pause,
                id: row.id,
            });
        }

        // progress bar
        let bar_y = y + px(CARD_H - 14.0);
        let bar_right = wpx - px(PAD + CARD_PAD + 3.0 * BTN_W + 48.0);
        let bar = rectf(px(PAD + CARD_PAD), bar_y, bar_right, bar_y + px(8.0));
        data.d2d.rounded(rt, bar, px(4.0), &b.track);
        let frac = if row.size > 0 {
            (row.done as f32 / row.size as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if frac > 0.005 {
            let fill_w = (bar_right - px(PAD + CARD_PAD)) * frac;
            let fill = rectf(
                px(PAD + CARD_PAD),
                bar_y,
                px(PAD + CARD_PAD) + fill_w,
                bar_y + px(8.0),
            );
            data.d2d.rounded(rt, fill, px(4.0), &b.primary);
        }

        // error / detail line
        if let Some(err) = &row.error {
            data.d2d.text(
                rt,
                err,
                &stat_tf,
                rectf(
                    px(PAD + CARD_PAD),
                    y + px(CARD_H - 32.0),
                    wpx - px(PAD + CARD_PAD),
                    y + px(CARD_H - 16.0),
                ),
                &b.error,
            );
        } else if !row.out_path.is_empty() && row.state == TaskState::Completed {
            data.d2d.text(
                rt,
                &row.out_path,
                &stat_tf,
                rectf(
                    px(PAD + CARD_PAD),
                    y + px(CARD_H - 32.0),
                    wpx - px(PAD + CARD_PAD),
                    y + px(CARD_H - 16.0),
                ),
                &b.on_surface_variant,
            );
        }

        y += px(CARD_H + CARD_GAP);
    }
}

// ---------------------------------------------------------------------------
// Window procedure
// ---------------------------------------------------------------------------

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            with_data(|data| paint(data, hwnd));
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_TIMER => {
            with_data(|data| data.app.refresh());
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_SIZE => {
            // Client size is in lparam (wparam carries SIZE_* flags).
            let w = (lparam.0 & 0xFFFF) as u32;
            let h = ((lparam.0 >> 16) & 0xFFFF) as u32;
            with_data(|data| {
                if let Some(rt) = &data.d2d.rt {
                    unsafe {
                        let _ = rt.Resize(&D2D_SIZE_U {
                            width: w.max(1),
                            height: h.max(1),
                        });
                    }
                }
                data.app.dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
                layout_children(&data.app, &client_rect(hwnd));
            });
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED => {
            with_data(|data| {
                data.app.dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
                layout_children(&data.app, &client_rect(hwnd));
            });
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i32;
            with_data(|data| {
                let zones = data.app.zones.clone();
                for zone in zones {
                    let (r, id, act) = match zone {
                        Zone::Pause { r, id } => (r, id, 0u8),
                        Zone::Resume { r, id } => (r, id, 1),
                        Zone::Remove { r, id } => (r, id, 2),
                    };
                    if x as f32 >= r.left
                        && x as f32 <= r.right
                        && y as f32 >= r.top
                        && y as f32 <= r.bottom
                    {
                        match act {
                            0 => {
                                let _ = data.app.engine.pause(id);
                            }
                            1 => {
                                let _ = data.app.engine.resume(id);
                            }
                            _ => {
                                let _ = data.app.engine.remove(id, false);
                            }
                        }
                        break;
                    }
                }
            });
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            let code = ((wparam.0 >> 16) & 0xFFFF) as i32;
            if code == BN_CLICKED as i32 {
                with_data(|data| match id {
                    IDC_BTN_ADD => add_download(&mut data.app),
                    IDC_BTN_SETTINGS => {
                        data.app.show_settings = !data.app.show_settings;
                        sync_settings_visibility(&data.app);
                        let cfg = data.app.engine.config();
                        push_settings(&data.app, &cfg);
                        layout_children(&data.app, &client_rect(hwnd));
                    }
                    IDC_BTN_SEED => {
                        let idx = vdm_md3::SEEDS
                            .iter()
                            .position(|(s, _)| *s == data.app.prefs.seed)
                            .unwrap_or(0);
                        let (next, _) = vdm_md3::SEEDS[(idx + 1) % vdm_md3::SEEDS.len()];
                        data.app.prefs.seed = next;
                        data.app.retheme();
                        config::save_prefs(&data.app.prefs, data.app.portable);
                        set_text(data.app.btn_seed, vdm_md3::seed_name(next));
                    }
                    IDC_BTN_DARK => {
                        data.app.prefs.dark = !data.app.prefs.dark;
                        data.app.retheme();
                        config::save_prefs(&data.app.prefs, data.app.portable);
                        set_text(
                            data.app.btn_dark,
                            if data.app.prefs.dark { "Dark" } else { "Light" },
                        );
                    }
                    IDC_BTN_APPLY => apply_settings(&mut data.app),
                    _ => {}
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> std::process::ExitCode {
    let portable = std::env::args().any(|a| a == "--portable");
    let addr = "127.0.0.1:36990";
    let cfg = config::load_config(portable);
    let engine = Engine::new(cfg.clone());
    let server = match RpcServer::start(engine.clone(), addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot bind {addr} ({e}); is another VelocityDM frontend running?");
            return std::process::ExitCode::FAILURE;
        }
    };
    let prefs = config::load_prefs(portable);
    let scheme = vdm_md3::Scheme::from_seed_hex(prefs.seed, prefs.dark);

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hmodule = match instance() {
            Ok(h) => h,
            Err(e) => {
                eprintln!("GetModuleHandleW: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        let hinstance = windows::Win32::Foundation::HINSTANCE::from(hmodule);
        let wc = WNDCLASSW {
            style: Default::default(),
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: w!("VelocityDMWin32"),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            eprintln!("RegisterClassW failed");
            return std::process::ExitCode::FAILURE;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("VelocityDMWin32"),
            w!("VelocityDM"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            980,
            640,
            None,
            None,
            Some(hinstance),
            None,
        ) else {
            eprintln!("CreateWindowExW failed");
            return std::process::ExitCode::FAILURE;
        };

        let mk_err = |what: &str, e: windows::core::Error| -> std::process::ExitCode {
            eprintln!("{what}: {e}");
            std::process::ExitCode::FAILURE
        };

        let Ok(edit_url) = make_edit(hwnd, 16, 72, 700, 24, IDC_EDIT_URL) else {
            return mk_err("EDIT url", windows::core::Error::empty());
        };
        let Ok(edit_quality) = make_edit(hwnd, 730, 72, 64, 24, IDC_EDIT_QUALITY) else {
            return mk_err("EDIT quality", windows::core::Error::empty());
        };
        let Ok(edit_limit) = make_edit(hwnd, 800, 72, 64, 24, IDC_EDIT_LIMIT) else {
            return mk_err("EDIT limit", windows::core::Error::empty());
        };
        let Ok(btn_add) = make_btn(hwnd, 876, 70, 72, 28, IDC_BTN_ADD, "Add", false) else {
            return mk_err("BUTTON add", windows::core::Error::empty());
        };
        let Ok(btn_settings) = make_btn(hwnd, 240, 14, 84, 28, IDC_BTN_SETTINGS, "Settings", false)
        else {
            return mk_err("BUTTON settings", windows::core::Error::empty());
        };
        let Ok(btn_seed) = make_btn(
            hwnd,
            336,
            14,
            120,
            28,
            IDC_BTN_SEED,
            vdm_md3::seed_name(prefs.seed),
            false,
        ) else {
            return mk_err("BUTTON seed", windows::core::Error::empty());
        };
        let Ok(btn_dark) = make_btn(
            hwnd,
            466,
            14,
            64,
            28,
            IDC_BTN_DARK,
            if prefs.dark { "Dark" } else { "Light" },
            false,
        ) else {
            return mk_err("BUTTON dark", windows::core::Error::empty());
        };
        let Ok(edit_dir) = make_edit(hwnd, 24, 96, 190, 24, IDC_EDIT_DIR) else {
            return mk_err("EDIT dir", windows::core::Error::empty());
        };
        let Ok(edit_conn) = make_edit(hwnd, 226, 96, 52, 24, IDC_EDIT_CONN) else {
            return mk_err("EDIT conn", windows::core::Error::empty());
        };
        let Ok(edit_max_active) = make_edit(hwnd, 288, 96, 52, 24, IDC_EDIT_MAX_ACTIVE) else {
            return mk_err("EDIT max", windows::core::Error::empty());
        };
        let Ok(edit_glimit) = make_edit(hwnd, 350, 96, 80, 24, IDC_EDIT_GLIMIT) else {
            return mk_err("EDIT glimit", windows::core::Error::empty());
        };
        let Ok(edit_proxy) = make_edit(hwnd, 440, 96, 200, 24, IDC_EDIT_PROXY) else {
            return mk_err("EDIT proxy", windows::core::Error::empty());
        };
        let Ok(edit_stop_start) = make_edit(hwnd, 24, 136, 56, 24, IDC_EDIT_STOP_START) else {
            return mk_err("EDIT stop-start", windows::core::Error::empty());
        };
        let Ok(edit_stop_end) = make_edit(hwnd, 90, 136, 56, 24, IDC_EDIT_STOP_END) else {
            return mk_err("EDIT stop-end", windows::core::Error::empty());
        };
        let Ok(chk_shutdown) = make_btn(
            hwnd,
            160,
            136,
            190,
            24,
            IDC_CHK_SHUTDOWN,
            "Shut down when queue drains",
            true,
        ) else {
            return mk_err("CHECK shutdown", windows::core::Error::empty());
        };
        let Ok(btn_apply) = make_btn(hwnd, 360, 134, 80, 28, IDC_BTN_APPLY, "Apply", false) else {
            return mk_err("BUTTON apply", windows::core::Error::empty());
        };

        let Ok(d2d) = D2d::new() else {
            eprintln!("Direct2D init failed (is D2D1.DLL available?)");
            return std::process::ExitCode::FAILURE;
        };

        let mut app = App {
            engine: engine.clone(),
            scheme,
            prefs,
            rows: Vec::new(),
            total_speed: 0,
            show_settings: false,
            portable,
            dpi: GetDpiForWindow(hwnd) as f32,
            edit_url,
            edit_quality,
            edit_limit,
            edit_dir,
            edit_conn,
            edit_max_active,
            edit_glimit,
            edit_proxy,
            edit_stop_start,
            edit_stop_end,
            chk_shutdown,
            btn_add,
            btn_settings,
            btn_seed,
            btn_dark,
            btn_apply,
            zones: Vec::new(),
        };
        app.refresh();
        push_settings(&app, &cfg);
        set_text(app.edit_quality, "best");
        layout_children(&app, &client_rect(hwnd));
        sync_settings_visibility(&app);

        WND.with(|slot| {
            *slot.borrow_mut() = Some(Rc::new(RefCell::new(WndData { app, d2d })));
        });

        SetTimer(Some(hwnd), 1, REFRESH_MILLIS, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    engine.shutdown();
    server.stop();
    std::process::ExitCode::SUCCESS
}

// keep unused imports referenced (WNDPROC is used via Some(wndproc) above;
// WINDOW_STYLE alias kept for readability of style or-combinations)
const _: Option<WNDPROC> = None;
