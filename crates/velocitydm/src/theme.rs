//! Material You (Material Design 3) theming for the egui GUI.
//!
//! Implements clean-room MD3 dynamic color: an OKLCH tonal-palette generator
//! (the OKLab math is Björn Ottosson's public-domain formulation), the MD3
//! scheme role table for light and dark appearances, and an egui style
//! applier. A seed color produces five tonal palettes (primary, secondary,
//! tertiary, neutral, neutral-variant) exactly like Material You dynamic
//! color, so the whole UI follows one accent hue.

use eframe::egui::{self, Color32, Visuals};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// OKLab / OKLCH <-> sRGB
// ---------------------------------------------------------------------------

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (0..1 floats) -> OKLab L (0..1), a, b.
fn linear_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = 0.412_221_4 * r + 0.536_332_5 * g + 0.051_445_99 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_95 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    (
        0.210_454_3 * l_ + 0.793_617_8 * m_ - 0.004_072_05 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_0 * l_ + 0.782_771_8 * m_ - 0.808_675_8 * s_,
    )
}

/// OKLab -> linear sRGB (may be out of gamut).
fn oklab_to_linear(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
    (
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_9 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 + 0.341_319_4 * s3,
        -0.004_196_09 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    )
}

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

fn in_gamut(r: f32, g: f32, b: f32) -> bool {
    (-1e-4..=1.0 + 1e-4).contains(&r)
        && (-1e-4..=1.0 + 1e-4).contains(&g)
        && (-1e-4..=1.0 + 1e-4).contains(&b)
}

/// OKLCH (L 0..1, C, hue in degrees) -> `Color32`, chroma reduced as needed
/// to stay inside the sRGB gamut (the same approach Material's HCT uses).
fn oklch_to_color(l: f32, mut c: f32, h_deg: f32) -> Color32 {
    let rad = h_deg.to_radians();
    let (a0, b0) = (c * rad.cos(), c * rad.sin());
    let (lr, lg, lb) = oklab_to_linear(l, a0, b0);
    if !in_gamut(lr, lg, lb) {
        // binary search the largest in-gamut chroma
        let (mut lo, mut hi) = (0.0_f32, c);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            let (a, b) = (mid * rad.cos(), mid * rad.sin());
            let (r, g, bl) = oklab_to_linear(l, a, b);
            if in_gamut(r, g, bl) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        c = lo;
    }
    let (a, b) = (c * rad.cos(), c * rad.sin());
    let (r, g, bl) = oklab_to_linear(l, a, b);
    Color32::from_rgb(
        (linear_to_srgb(clamp01(r)) * 255.0).round() as u8,
        (linear_to_srgb(clamp01(g)) * 255.0).round() as u8,
        (linear_to_srgb(clamp01(bl)) * 255.0).round() as u8,
    )
}

fn color_to_oklch(c: Color32) -> (f32, f32, f32) {
    let (r, g, b) = (
        srgb_to_linear(c.r() as f32 / 255.0),
        srgb_to_linear(c.g() as f32 / 255.0),
        srgb_to_linear(c.b() as f32 / 255.0),
    );
    let (l, a, b) = linear_to_oklab(r, g, b);
    let chroma = (a * a + b * b).sqrt();
    let mut hue = b.atan2(a).to_degrees();
    if hue < 0.0 {
        hue += 360.0;
    }
    (l, chroma, hue)
}

// ---------------------------------------------------------------------------
// Tonal palettes (MD3: a hue + chroma, sampled at tones 0..100)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Palette {
    hue: f32,
    chroma: f32,
}

impl Palette {
    const fn new(hue: f32, chroma: f32) -> Self {
        Self { hue, chroma }
    }

    /// Tone 0..100 -> color (tone == OKLab L in percent).
    fn tone(&self, t: f32) -> Color32 {
        oklch_to_color(t / 100.0, self.chroma, self.hue)
    }
}

// ---------------------------------------------------------------------------
// Scheme (MD3 color roles)
// ---------------------------------------------------------------------------

/// Full Material Design 3 color-role set generated from one seed color.
/// Some roles (inverse/scrim/on-error) are defined for completeness even
/// where the current widgets use alternatives.
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
    /// Generate the light or dark scheme for a seed color.
    pub fn from_seed(seed: Color32, dark: bool) -> Scheme {
        let (_, c, h) = color_to_oklch(seed);
        // Chroma boost: a dull seed still yields a vivid accent, as in
        // Material You's "source color" processing.
        let boost = c.max(if dark { 0.115 } else { 0.13 });
        let primary = Palette::new(h, boost);
        let secondary = Palette::new(h, (boost * 0.34).max(0.028));
        let tertiary = Palette::new((h + 52.0) % 360.0, (boost * 0.72).max(0.075));
        let neutral = Palette::new(h, 0.006);
        let neutral_variant = Palette::new(h, 0.014);
        let error = Palette::new(27.0, 0.155);

        if dark {
            Scheme {
                dark,
                primary: primary.tone(80.0),
                on_primary: primary.tone(20.0),
                primary_container: primary.tone(30.0),
                on_primary_container: primary.tone(90.0),
                secondary_container: secondary.tone(30.0),
                on_secondary_container: secondary.tone(90.0),
                tertiary: tertiary.tone(80.0),
                on_tertiary_container: tertiary.tone(90.0),
                tertiary_container: tertiary.tone(30.0),
                error: error.tone(80.0),
                on_error: error.tone(20.0),
                error_container: error.tone(30.0),
                on_error_container: error.tone(90.0),
                surface: neutral.tone(6.0),
                surface_container_lowest: neutral.tone(4.0),
                surface_container_low: neutral.tone(10.0),
                surface_container: neutral.tone(12.0),
                surface_container_high: neutral.tone(17.0),
                surface_container_highest: neutral.tone(22.0),
                on_surface: neutral.tone(90.0),
                on_surface_variant: neutral_variant.tone(80.0),
                outline: neutral_variant.tone(60.0),
                outline_variant: neutral_variant.tone(30.0),
                inverse_surface: neutral.tone(90.0),
                inverse_on_surface: neutral.tone(20.0),
                inverse_primary: primary.tone(40.0),
                scrim: neutral.tone(0.0),
            }
        } else {
            Scheme {
                dark,
                primary: primary.tone(40.0),
                on_primary: primary.tone(100.0),
                primary_container: primary.tone(90.0),
                on_primary_container: primary.tone(10.0),
                secondary_container: secondary.tone(90.0),
                on_secondary_container: secondary.tone(10.0),
                tertiary: tertiary.tone(40.0),
                on_tertiary_container: tertiary.tone(10.0),
                tertiary_container: tertiary.tone(90.0),
                error: error.tone(40.0),
                on_error: error.tone(100.0),
                error_container: error.tone(90.0),
                on_error_container: error.tone(10.0),
                surface: neutral.tone(98.0),
                surface_container_lowest: neutral.tone(100.0),
                surface_container_low: neutral.tone(96.0),
                surface_container: neutral.tone(94.0),
                surface_container_high: neutral.tone(92.0),
                surface_container_highest: neutral.tone(90.0),
                on_surface: neutral.tone(10.0),
                on_surface_variant: neutral_variant.tone(30.0),
                outline: neutral_variant.tone(50.0),
                outline_variant: neutral_variant.tone(80.0),
                inverse_surface: neutral.tone(20.0),
                inverse_on_surface: neutral.tone(95.0),
                inverse_primary: primary.tone(80.0),
                scrim: neutral_variant.tone(0.0),
            }
        }
    }

    /// Build with an explicit hex seed (0xRRGGBB).
    pub fn from_seed_hex(seed_hex: u32, dark: bool) -> Scheme {
        Scheme::from_seed(
            Color32::from_rgb(
                (seed_hex >> 16) as u8,
                (seed_hex >> 8) as u8,
                seed_hex as u8,
            ),
            dark,
        )
    }
}

// ---------------------------------------------------------------------------
// Material You seed choices shown in the settings panel
// ---------------------------------------------------------------------------

/// Named seed colors (default Material baseline purple first).
pub const SEEDS: &[(u32, &str)] = &[
    (0x67_50_A4, "Baseline purple"),
    (0x4C_7D_11, "Green"),
    (0x0B_57_CF, "Blue"),
    (0xB3_26_1E, "Red"),
    (0x7D_52_60, "Rose"),
    (0x8A_64_00, "Amber"),
];

pub fn seed_name(seed_hex: u32) -> &'static str {
    SEEDS
        .iter()
        .find(|(s, _)| *s == seed_hex)
        .map(|(_, n)| *n)
        .unwrap_or("Custom")
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
            seed: SEEDS[0].0,
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
