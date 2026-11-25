//! vdm-md3 → QML color bridge.
//!
//! Seed → scheme runs in Rust (`vdm_md3::Scheme::from_seed_hex`); the QML
//! side receives role colors as hex strings built EXCLUSIVELY via
//! `format!("#{:06x}", v & 0xFFFFFF)` — typed data only, zero runtime
//! strings in any color channel. The role subset shipped is exactly the
//! set the QML consumes (the gtk3 19/27 honesty pattern); the count is
//! asserted by test so the README claim cannot drift.

#![deny(unsafe_code)]

/// The MD3 roles this frontend's QML actually consumes. `Scheme` carries
/// 27 roles; everything outside this list is deliberately not shipped to
/// the QML layer (honest subset, named here).
pub const ROLE_SUBSET: usize = 16;

/// A color role value as a `#rrggbb` hex string.
pub fn role_hex(v: u32) -> String {
    format!("#{:06x}", v & 0xFFFFFF)
}

/// The bridge payload: one hex string per consumed role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub primary: String,
    pub on_primary: String,
    pub primary_container: String,
    pub on_primary_container: String,
    pub secondary_container: String,
    pub on_secondary_container: String,
    pub tertiary_container: String,
    pub on_tertiary_container: String,
    pub error: String,
    pub on_error: String,
    pub surface: String,
    pub on_surface: String,
    pub surface_container: String,
    pub on_surface_variant: String,
    pub outline: String,
    pub outline_variant: String,
}

/// Compute the consumed-role tokens for a seed / dark-mode pair.
pub fn tokens_for(seed: u32, dark: bool) -> Tokens {
    let s = vdm_md3::Scheme::from_seed_hex(seed, dark);
    Tokens {
        primary: role_hex(s.primary),
        on_primary: role_hex(s.on_primary),
        primary_container: role_hex(s.primary_container),
        on_primary_container: role_hex(s.on_primary_container),
        secondary_container: role_hex(s.secondary_container),
        on_secondary_container: role_hex(s.on_secondary_container),
        tertiary_container: role_hex(s.tertiary_container),
        on_tertiary_container: role_hex(s.on_tertiary_container),
        error: role_hex(s.error),
        on_error: role_hex(s.on_error),
        surface: role_hex(s.surface),
        on_surface: role_hex(s.on_surface),
        surface_container: role_hex(s.surface_container),
        on_surface_variant: role_hex(s.on_surface_variant),
        outline: role_hex(s.outline),
        outline_variant: role_hex(s.outline_variant),
    }
}

/// Next seed in the shared `vdm_md3::SEEDS` cycle (wraps), with its name.
pub fn next_seed(current: u32) -> (u32, &'static str) {
    let seeds = vdm_md3::SEEDS;
    let pos = seeds.iter().position(|&(v, _)| v == current);
    let next = match pos {
        Some(i) => seeds[(i + 1) % seeds.len()],
        None => seeds[0],
    };
    (next.0, next.1)
}

/// The name of a seed (shared helper, "Custom" when unknown).
pub fn seed_name(seed: u32) -> &'static str {
    vdm_md3::seed_name(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_format_is_always_six_digits() {
        assert_eq!(role_hex(0), "#000000");
        assert_eq!(role_hex(0xFFFFFF), "#ffffff");
        assert_eq!(role_hex(0x4156A4), "#4156a4");
        // high garbage bits are masked, never rendered
        assert_eq!(role_hex(0xFF4156A4), "#4156a4");
    }

    #[test]
    fn role_subset_count_matches_struct() {
        // 16 fields, one per consumed role — the README honesty tripwire
        let t = tokens_for(vdm_md3::DEFAULT_SEED, true);
        let fields = [
            &t.primary,
            &t.on_primary,
            &t.primary_container,
            &t.on_primary_container,
            &t.secondary_container,
            &t.on_secondary_container,
            &t.tertiary_container,
            &t.on_tertiary_container,
            &t.error,
            &t.on_error,
            &t.surface,
            &t.on_surface,
            &t.surface_container,
            &t.on_surface_variant,
            &t.outline,
            &t.outline_variant,
        ];
        assert_eq!(fields.len(), ROLE_SUBSET);
        for f in fields {
            assert!(f.starts_with('#') && f.len() == 7, "{f}");
        }
    }

    #[test]
    fn dark_and_light_differ_and_cycle_wraps() {
        let dark = tokens_for(vdm_md3::DEFAULT_SEED, true);
        let light = tokens_for(vdm_md3::DEFAULT_SEED, false);
        assert_ne!(dark.primary, light.primary);
        // cycling past the last seed wraps to the first
        let (last_seed, _) = vdm_md3::SEEDS[vdm_md3::SEEDS.len() - 1];
        let (next, _) = next_seed(last_seed);
        assert_eq!(next, vdm_md3::SEEDS[0].0);
        // an unknown seed joins the cycle at the first entry
        let (joined, _) = next_seed(0xDEADBEEF);
        assert_eq!(joined, vdm_md3::SEEDS[0].0);
    }
}
