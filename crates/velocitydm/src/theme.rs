//! Material You (MD3) theming for the egui GUI.
//!
//! Thin adapter over the shared `vdm-md3` token engine (the same
//! engine every VelocityDM frontend uses — Win32/Direct2D, Slint, the
//! extension picker, the Kotlin port), plus the egui style applier and
//! persisted UI preferences.

use eframe::egui::{self, Color32, Visuals};
use std::path::PathBuf;

/// Re-exported so settings surfaces iterate one canonical list.
pub use vdm_md3::{seed_name, SEEDS};

fn c32(hex: u32) -> Color32 {
    Color32::from_rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

/// Full MD3 color-role set as egui colors (see `vdm_md3::Scheme` for
/// the shared, serializable hex-token form).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct Scheme {
    pub dark: bool,
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub tertiary: Color32,
    pub on_tertiary_container: Color32,
    pub tertiary_container: Color32,
    pub error: Color32,
    pub on_error: Color32,
    pub error_container: Color32,
    pub on_error_container: Color32,
    pub surface: Color32,
    pub surface_container_lowest: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
    pub inverse_surface: Color32,
    pub inverse_on_surface: Color32,
    pub inverse_primary: Color32,
    pub scrim: Color32,
}

impl Scheme {
    /// Build with an explicit hex seed (0xRRGGBB).
    pub fn from_seed_hex(seed_hex: u32, dark: bool) -> Scheme {
        Scheme::from_tokens(vdm_md3::Scheme::from_seed_hex(seed_hex, dark))
    }

    fn from_tokens(s: vdm_md3::Scheme) -> Scheme {
        Scheme {
            dark: s.dark,
            primary: c32(s.primary),
            on_primary: c32(s.on_primary),
            primary_container: c32(s.primary_container),
            on_primary_container: c32(s.on_primary_container),
            secondary_container: c32(s.secondary_container),
            on_secondary_container: c32(s.on_secondary_container),
            tertiary: c32(s.tertiary),
            on_tertiary_container: c32(s.on_tertiary_container),
            tertiary_container: c32(s.tertiary_container),
            error: c32(s.error),
            on_error: c32(s.on_error),
            error_container: c32(s.error_container),
            on_error_container: c32(s.on_error_container),
            surface: c32(s.surface),
            surface_container_lowest: c32(s.surface_container_lowest),
            surface_container_low: c32(s.surface_container_low),
            surface_container: c32(s.surface_container),
            surface_container_high: c32(s.surface_container_high),
            surface_container_highest: c32(s.surface_container_highest),
            on_surface: c32(s.on_surface),
            on_surface_variant: c32(s.on_surface_variant),
            outline: c32(s.outline),
            outline_variant: c32(s.outline_variant),
            inverse_surface: c32(s.inverse_surface),
            inverse_on_surface: c32(s.inverse_on_surface),
            inverse_primary: c32(s.inverse_primary),
            scrim: c32(s.scrim),
        }
    }
}

// ---------------------------------------------------------------------------
// egui style applier
// ---------------------------------------------------------------------------

/// Push the scheme into the egui style (surfaces, widgets, selection, text).
pub fn apply(ctx: &egui::Context, s: &Scheme) {
    let mut style = (*ctx.style()).clone();
    let mut v = if s.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = s.surface;
    v.window_fill = s.surface_container;
    v.extreme_bg_color = s.surface_container_lowest;
    v.faint_bg_color = s.surface_container_low;
    v.button_frame = true;
    v.window_rounding = egui::Rounding::same(16.0);
    v.menu_rounding = egui::Rounding::same(12.0);

    // MD3 widget states: idle = surface container, hover = high, active = higher.
    for (w, fill, text, stroke) in [
        (
            &mut v.widgets.inactive,
            s.surface_container,
            s.on_surface,
            s.outline_variant,
        ),
        (
            &mut v.widgets.hovered,
            s.surface_container_high,
            s.on_surface,
            s.outline,
        ),
        (
            &mut v.widgets.active,
            s.surface_container_highest,
            s.on_surface,
            s.outline,
        ),
        (
            &mut v.widgets.noninteractive,
            Color32::TRANSPARENT,
            s.on_surface_variant,
            s.outline_variant,
        ),
        (
            &mut v.widgets.open,
            s.surface_container_high,
            s.on_surface,
            s.outline,
        ),
    ] {
        w.bg_fill = fill;
        w.fg_stroke = egui::Stroke::new(1.0_f32, text);
        w.bg_stroke = egui::Stroke::new(1.0_f32, stroke);
        w.rounding = egui::Rounding::same(8.0);
    }
    v.selection.bg_fill = s.primary_container;
    v.selection.stroke = egui::Stroke::new(1.0_f32, s.on_primary_container);
    v.widgets.inactive.rounding = egui::Rounding::same(9999.0);
    v.widgets.hovered.rounding = egui::Rounding::same(9999.0);
    v.widgets.active.rounding = egui::Rounding::same(9999.0);

    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(16.0, 8.0);
    style.spacing.icon_width_inner = 18.0;
    if let Some(font_id) = style.text_styles.get_mut(&egui::TextStyle::Body) {
        font_id.size = 14.0;
    }
    if let Some(font_id) = style.text_styles.get_mut(&egui::TextStyle::Heading) {
        font_id.size = 22.0;
    }
    if let Some(font_id) = style.text_styles.get_mut(&egui::TextStyle::Button) {
        font_id.size = 14.0;
    }
    ctx.set_style(style);
}

// ---------------------------------------------------------------------------
// UI preferences (persisted next to the daemon config)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UiPrefs {
    /// Seed color, 0xRRGGBB.
    pub seed: u32,
    pub dark: bool,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            seed: vdm_md3::DEFAULT_SEED,
            dark: true,
        }
    }
}

fn prefs_path(portable: bool) -> PathBuf {
    let cfg = crate::daemon::config_path(portable);
    cfg.parent()
        .map(|d| d.join("ui.json"))
        .unwrap_or_else(|| PathBuf::from("velocitydm.ui.json"))
}

pub fn load_prefs(portable: bool) -> UiPrefs {
    if let Ok(text) = std::fs::read_to_string(prefs_path(portable)) {
        if let Ok(p) = serde_json::from_str::<UiPrefs>(&text) {
            return p;
        }
    }
    UiPrefs::default()
}

pub fn save_prefs(p: &UiPrefs, portable: bool) {
    let path = prefs_path(portable);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(p) {
        let _ = std::fs::write(path, text);
    }
}
