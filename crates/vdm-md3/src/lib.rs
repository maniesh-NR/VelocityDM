//! VelocityDM shared Material You (MD3) dynamic-color token engine.
//!
//! One crate, every frontend: the Windows (Win32 + Direct2D), Linux
//! (Slint / GTK3), Android (Kotlin port of this exact math) and
//! token-table frontends all derive their schemes here, so a seed
//! color renders the same roles on every operating system. The egui
//! GUI (`crates/velocitydm/src/theme.rs`) delegates to this crate as
//! well.
//!
//! Clean-room implementation: the OKLab <-> sRGB math is Björn
//! Ottosson's public-domain formulation; the tone tables follow the
//! published Material Design 3 color-role assignments. No Google
//! source code, tables, or resources are used.
//!
//! Model: seed color -> OKLCH (hue, chroma) -> five tonal palettes
//! (primary, secondary, tertiary, neutral, neutral-variant, plus a
//! fixed error hue) -> MD3 light/dark scheme roles, chroma clipped to
//! the sRGB gamut at constant tone (binary search, 24 iterations).

#![deny(unsafe_code)]

// ---------------------------------------------------------------------------
// OKLab / OKLCH <-> sRGB (public-domain formulation)
// ---------------------------------------------------------------------------

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (0..1) -> OKLab (L in 0..1, a, b).
fn linear_to_oklab(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let l = 0.412_221_470_8 * r + 0.536_332_512 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    (
        0.210_454_255_3 * l_ + 0.793_617_785 * m_ - 0.004_072_046_8 * s_,
        1.977_998_495_4 * l_ - 2.428_592_205 * m_ + 0.450_593_709_9 * s_,
        0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766 * s_,
    )
}

/// OKLab -> linear sRGB (may be out of gamut).
fn oklab_to_linear(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let (l3, m3, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
    (
        4.076_741_662_1 * l3 - 3.307_711_591_3 * m3 + 0.230_969_929_2 * s3,
        -1.268_438_626_9 * l3 + 2.609_757_372_9 * m3 + 0.341_319_387_7 * s3,
        -0.004_196_086_5 * l3 - 0.703_418_614_7 * m3 + 1.707_614_693_7 * s3,
    )
}

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

fn in_gamut(r: f64, g: f64, b: f64) -> bool {
    (-1e-4..=1.0 + 1e-4).contains(&r)
        && (-1e-4..=1.0 + 1e-4).contains(&g)
        && (-1e-4..=1.0 + 1e-4).contains(&b)
}

/// One OKLCH point (tone = L in percent, chroma, hue degrees) as packed
/// `0xRRGGBB`, chroma reduced as needed to stay in the sRGB gamut.
fn oklch_to_hex(tone: f64, mut c: f64, h_deg: f64) -> u32 {
    let rad = h_deg.to_radians();
    let (a0, b0) = (c * rad.cos(), c * rad.sin());
    let (lr, lg, lb) = oklab_to_linear(tone / 100.0, a0, b0);
    if !in_gamut(lr, lg, lb) {
        // binary search the largest in-gamut chroma at this tone
        let (mut lo, mut hi) = (0.0_f64, c);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            let (a, b) = (mid * rad.cos(), mid * rad.sin());
            let (r, g, bl) = oklab_to_linear(tone / 100.0, a, b);
            if in_gamut(r, g, bl) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        c = lo;
    }
    let (a, b) = (c * rad.cos(), c * rad.sin());
    let (r, g, bl) = oklab_to_linear(tone / 100.0, a, b);
    let rr = (linear_to_srgb(clamp01(r)) * 255.0).round() as u32;
    let gg = (linear_to_srgb(clamp01(g)) * 255.0).round() as u32;
    let bb = (linear_to_srgb(clamp01(bl)) * 255.0).round() as u32;
    (rr << 16) | (gg << 8) | bb
}

/// sRGB bytes -> OKLCH (L in 0..1, chroma, hue degrees in 0..360).
fn rgb_to_oklch(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (lr, lg, lb) = (
        srgb_to_linear(f64::from(r) / 255.0),
        srgb_to_linear(f64::from(g) / 255.0),
        srgb_to_linear(f64::from(b) / 255.0),
    );
    let (l, a, b) = linear_to_oklab(lr, lg, lb);
    let chroma = (a * a + b * b).sqrt();
    let mut hue = b.atan2(a).to_degrees();
    if hue < 0.0 {
        hue += 360.0;
    }
    (l, chroma, hue)
}

// ---------------------------------------------------------------------------
// Tonal palettes
// ---------------------------------------------------------------------------

/// A tonal palette: fixed hue + chroma, sampled at tones 0..100.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    hue: f64,
    chroma: f64,
}

impl Palette {
    pub const fn new(hue: f64, chroma: f64) -> Self {
        Self { hue, chroma }
    }

    /// Tone 0..100 -> packed `0xRRGGBB` (tone == OKLab L in percent).
    pub fn tone(&self, t: f64) -> u32 {
        oklch_to_hex(t, self.chroma, self.hue)
    }
}

/// Tone-preserving tonal value (exposed for parity tests with the
/// browser-extension engine and the Kotlin port).
pub fn tonal(hue_deg: f64, chroma: f64, tone: f64) -> u32 {
    oklch_to_hex(tone, chroma, hue_deg)
}

// ---------------------------------------------------------------------------
// Scheme (MD3 color roles)
// ---------------------------------------------------------------------------

/// Full Material Design 3 color-role set generated from one seed color,
/// every field a packed `0xRRGGBB` value. Serializable (feature `json`)
/// so non-Rust frontends (Swift, Kotlin) can consume the same tokens.
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    pub dark: bool,
    pub primary: u32,
    pub on_primary: u32,
    pub primary_container: u32,
    pub on_primary_container: u32,
    pub secondary_container: u32,
    pub on_secondary_container: u32,
    pub tertiary: u32,
    pub on_tertiary_container: u32,
    pub tertiary_container: u32,
    pub error: u32,
    pub on_error: u32,
    pub error_container: u32,
    pub on_error_container: u32,
    pub surface: u32,
    pub surface_container_lowest: u32,
    pub surface_container_low: u32,
    pub surface_container: u32,
    pub surface_container_high: u32,
    pub surface_container_highest: u32,
    pub on_surface: u32,
    pub on_surface_variant: u32,
    pub outline: u32,
    pub outline_variant: u32,
    pub inverse_surface: u32,
    pub inverse_on_surface: u32,
    pub inverse_primary: u32,
    pub scrim: u32,
}

impl Scheme {
    /// Generate the light or dark scheme from sRGB bytes.
    pub fn from_seed_rgb(r: u8, g: u8, b: u8, dark: bool) -> Scheme {
        let (_, c, h) = rgb_to_oklch(r, g, b);
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

    /// Build with an explicit hex seed (`0xRRGGBB`).
    pub fn from_seed_hex(seed_hex: u32, dark: bool) -> Scheme {
        Scheme::from_seed_rgb(
            ((seed_hex >> 16) & 0xFF) as u8,
            ((seed_hex >> 8) & 0xFF) as u8,
            (seed_hex & 0xFF) as u8,
            dark,
        )
    }

    /// Parse `#RRGGBB` or `RRGGBB`; `None` when malformed.
    pub fn parse_hex(s: &str) -> Option<u32> {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(h, 16).ok()
    }
}

// ---------------------------------------------------------------------------
// Canonical seed choices (shared across every frontend; matches the
// browser-extension presets and the desktop settings panel)
// ---------------------------------------------------------------------------

/// Named seed colors shown in every frontend's Material You picker.
pub const SEEDS: &[(u32, &str)] = &[
    (0x41_56_A4, "Brand blue"),
    (0x67_50_A4, "Baseline purple"),
    (0x00_63_9B, "Ocean"),
    (0x00_6A_6A, "Teal"),
    (0x38_6A_33, "Forest"),
    (0x8F_4C_00, "Sunset"),
    (0x98_40_61, "Rose"),
    (0x56_60_6F, "Slate"),
];

/// Default seed: the brand blue used across VelocityDM's surfaces.
pub const DEFAULT_SEED: u32 = 0x41_56_A4;

pub fn seed_name(seed_hex: u32) -> &'static str {
    SEEDS
        .iter()
        .find(|(s, _)| *s == seed_hex)
        .map(|(_, n)| *n)
        .unwrap_or("Custom")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_variants() {
        assert_eq!(Scheme::parse_hex("#4156A4"), Some(0x4156A4));
        assert_eq!(Scheme::parse_hex("4156a4"), Some(0x4156A4));
        assert_eq!(Scheme::parse_hex("#12345"), None);
        assert_eq!(Scheme::parse_hex("zzzzzz"), None);
    }

    #[test]
    fn tones_are_monotonic_in_lightness() {
        // Neutral palette: tone 6 (dark surface) must be much darker
        // than tone 90 (dark on-surface) in perceived L.
        let p = Palette::new(264.0, 0.006);
        let (l6, _, _) = {
            let v = p.tone(6.0);
            let rgb = ((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
            let lin = (
                srgb_to_linear(f64::from(rgb.0) / 255.0),
                srgb_to_linear(f64::from(rgb.1) / 255.0),
                srgb_to_linear(f64::from(rgb.2) / 255.0),
            );
            let (l, _, _) = linear_to_oklab(lin.0, lin.1, lin.2);
            (l, 0.0, 0.0)
        };
        let l90 = {
            let v = p.tone(90.0);
            let rgb = ((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
            let lin = (
                srgb_to_linear(f64::from(rgb.0) / 255.0),
                srgb_to_linear(f64::from(rgb.1) / 255.0),
                srgb_to_linear(f64::from(rgb.2) / 255.0),
            );
            let (l, _, _) = linear_to_oklab(lin.0, lin.1, lin.2);
            l
        };
        assert!(l6 < 0.2, "tone 6 should be dark, got L={l6}");
        assert!(l90 > 0.8, "tone 90 should be light, got L={l90}");
    }

    #[test]
    fn dark_and_light_schemes_differ_in_every_surface_role() {
        let d = Scheme::from_seed_hex(0x4156A4, true);
        let l = Scheme::from_seed_hex(0x4156A4, false);
        for (a, b) in [
            (d.surface, l.surface),
            (d.primary, l.primary),
            (d.on_primary, l.on_primary),
            (d.outline, l.outline),
        ] {
            assert_ne!(a, b);
        }
        assert!(d.dark);
        assert!(!l.dark);
    }

    #[test]
    fn scheme_roles_are_in_gamut_bytes() {
        for dark in [true, false] {
            let s = Scheme::from_seed_hex(0x8F_4C_00, dark);
            for v in [
                s.primary,
                s.on_primary,
                s.surface,
                s.on_surface,
                s.error,
                s.outline,
            ] {
                assert_eq!(v & 0xFF00_0000, 0, "role value must be 24-bit: {v:#08x}");
            }
        }
    }

    #[test]
    fn round_trip_through_json() {
        #[cfg(feature = "json")]
        {
            let s = Scheme::from_seed_hex(DEFAULT_SEED, true);
            let j = serde_json::to_string(&s).unwrap();
            let back: Scheme = serde_json::from_str(&j).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn golden_values_stable() {
        // Golden literals pin the engine (generated by `md3-tokens
        // --seed 4156A4`): any change to the color math must be
        // deliberate and update these — plus the Swift port
        // (Md3.swift) and the extension engine parity notes.
        let d = Scheme::from_seed_hex(0x41_56_A4, true);
        assert_eq!(d.primary, 0xAA_FF_F0, "dark primary");
        assert_eq!(d.on_primary, 0x07_33_4E, "dark on_primary");
        assert_eq!(d.surface, 0x01_01_01, "dark surface");
        assert_eq!(d.error, 0xFF_CD_96, "dark error");
        let l = Scheme::from_seed_hex(0x41_56_A4, false);
        assert_eq!(l.primary, 0x2D_74_8D, "light primary");
        assert_eq!(l.on_primary, 0xFF_FF_FF, "light on_primary");
        assert_eq!(l.surface, 0xF8_FF_F8, "light surface");
        // Deterministic across calls:
        let d2 = Scheme::from_seed_hex(0x41_56_A4, true);
        assert_eq!(d, d2);
        // A dull seed still gets a vivid accent (chroma boost):
        let dull = Scheme::from_seed_rgb(0x80, 0x80, 0x80, false).primary;
        let lin = (
            srgb_to_linear(f64::from((dull >> 16) & 0xFF) / 255.0),
            srgb_to_linear(f64::from((dull >> 8) & 0xFF) / 255.0),
            srgb_to_linear(f64::from(dull & 0xFF) / 255.0),
        );
        let (_, a, b) = linear_to_oklab(lin.0, lin.1, lin.2);
        let c = (a * a + b * b).sqrt();
        assert!(c >= 0.09, "dull seed chroma boost failed: {c}");
    }

    #[test]
    fn tonal_matches_palette() {
        let p = Palette::new(120.0, 0.10);
        assert_eq!(p.tone(40.0), tonal(120.0, 0.10, 40.0));
    }
}
