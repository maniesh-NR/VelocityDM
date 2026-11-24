// VelocityDM macOS frontend — Material You (MD3) dynamic color.
//
// Swift port of the shared `vdm-md3` token engine (crates/vdm-md3):
// identical OKLab <-> sRGB math (Björn Ottosson's public-domain
// formulation), identical tone tables — so the seed color renders the
// same roles on macOS as on every other platform. The Rust crate is
// the canonical engine; this port exists so the AppKit frontend themes
// itself without a build step.

import AppKit

// MARK: - OKLab / OKLCH <-> sRGB (public-domain formulation)

private func srgbToLinear(_ c: Double) -> Double {
    c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
}

private func linearToSrgb(_ c: Double) -> Double {
    c <= 0.0031308 ? c * 12.92 : 1.055 * pow(c, 1.0 / 2.4) - 0.055
}

private func linearToOklab(r: Double, g: Double, b: Double) -> (Double, Double, Double) {
    let l = 0.4122214708 * r + 0.536332512 * g + 0.0514459929 * b
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    let l_ = cbrt(l), m_ = cbrt(m), s_ = cbrt(s)
    return (
        0.2104542553 * l_ + 0.793617785 * m_ - 0.0040720468 * s_,
        1.9779984954 * l_ - 2.428592205 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.808675766 * s_
    )
}

private func oklabToLinear(_ l: Double, _ a: Double, _ b: Double) -> (Double, Double, Double) {
    let l_ = l + 0.3963377774 * a + 0.2158037573 * b
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b
    let s_ = l - 0.0894841775 * a - 1.291485548 * b
    let l3 = l_ * l_ * l_, m3 = m_ * m_ * m_, s3 = s_ * s_ * s_
    return (
        4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3,
        -1.2684386269 * l3 + 2.6097573729 * m3 + 0.3413193877 * s3,
        -0.0041960865 * l3 - 0.7034186147 * m3 + 1.7076146937 * s3
    )
}

private func inGamut(_ r: Double, _ g: Double, _ b: Double) -> Bool {
    (r >= -1e-4 && r <= 1.0 + 1e-4)
        && (g >= -1e-4 && g <= 1.0 + 1e-4)
        && (b >= -1e-4 && b <= 1.0 + 1e-4)
}

/// One OKLCH point (tone = L in percent) as packed 0xRRGGBB, chroma
/// clipped into the sRGB gamut by binary search (24 iterations).
private func oklchToHex(tone: Double, chroma cIn: Double, hueDeg: Double) -> UInt32 {
    let rad = hueDeg * Double.pi / 180.0
    var c = cIn
    let a0 = c * cos(rad), b0 = c * sin(rad)
    let out = oklabToLinear(tone / 100.0, a0, b0)
    if !inGamut(out.0, out.1, out.2) {
        var lo = 0.0, hi = c
        for _ in 0..<24 {
            let mid = (lo + hi) / 2.0
            let t = oklabToLinear(tone / 100.0, mid * cos(rad), mid * sin(rad))
            if inGamut(t.0, t.1, t.2) { lo = mid } else { hi = mid }
        }
        c = lo
    }
    let fin = oklabToLinear(tone / 100.0, c * cos(rad), c * sin(rad))
    let r = Int((linearToSrgb(min(1, max(0, fin.0))) * 255.0).rounded())
    let g = Int((linearToSrgb(min(1, max(0, fin.1))) * 255.0).rounded())
    let b = Int((linearToSrgb(min(1, max(0, fin.2))) * 255.0).rounded())
    return (UInt32(r) << 16) | (UInt32(g) << 8) | UInt32(b)
}

private func rgbToOklch(r: UInt8, g: UInt8, b: UInt8) -> (Double, Double, Double) {
    let lr = srgbToLinear(Double(r) / 255.0)
    let lg = srgbToLinear(Double(g) / 255.0)
    let lb = srgbToLinear(Double(b) / 255.0)
    let (l, a, b) = linearToOklab(r: lr, g: lg, b: lb)
    let chroma = (a * a + b * b).squareRoot()
    var hue = atan2(b, a) * 180.0 / Double.pi
    if hue < 0 { hue += 360.0 }
    return (l, chroma, hue)
}

// MARK: - Scheme (MD3 color roles)

/// Material Design 3 color roles generated from one seed color — 18 of
/// the 27 `vdm_md3::Scheme` roles (missing: onTertiaryContainer,
/// tertiaryContainer, onError, errorContainer, onErrorContainer,
/// inverseSurface, inverseOnSurface, inversePrimary, scrim). This is
/// the honest subset this port carries — NOT full token parity (the
/// falsified "full color-role set" claim was corrected by the S12-mac2
/// header-truth commit, gate 0013 rule 1.2 exception).
public struct Md3Scheme {
    public let dark: Bool
    public let primary: UInt32
    public let onPrimary: UInt32
    public let primaryContainer: UInt32
    public let onPrimaryContainer: UInt32
    public let secondaryContainer: UInt32
    public let onSecondaryContainer: UInt32
    public let tertiary: UInt32
    public let error: UInt32
    public let surface: UInt32
    public let surfaceContainerLowest: UInt32
    public let surfaceContainerLow: UInt32
    public let surfaceContainer: UInt32
    public let surfaceContainerHigh: UInt32
    public let surfaceContainerHighest: UInt32
    public let onSurface: UInt32
    public let onSurfaceVariant: UInt32
    public let outline: UInt32
    public let outlineVariant: UInt32

    public static func fromSeedHex(_ seedHex: UInt32, dark: Bool) -> Md3Scheme {
        let r = UInt8((seedHex >> 16) & 0xFF)
        let g = UInt8((seedHex >> 8) & 0xFF)
        let b = UInt8(seedHex & 0xFF)
        let (_, c, h) = rgbToOklch(r: r, g: g, b: b)
        let boost = max(c, dark ? 0.115 : 0.13)
        func palette(_ hue: Double, _ chroma: Double) -> (Double) -> UInt32 {
            { tone in oklchToHex(tone: tone, chroma: chroma, hueDeg: hue) }
        }
        let primary = palette(h, boost)
        let secondary = palette(h, max(boost * 0.34, 0.028))
        let tertiary = palette(fmod(h + 52.0, 360.0), max(boost * 0.72, 0.075))
        let neutral = palette(h, 0.006)
        let neutralVariant = palette(h, 0.014)
        let error = palette(27.0, 0.155)

        if dark {
            return Md3Scheme(
                dark: true,
                primary: primary(80), onPrimary: primary(20),
                primaryContainer: primary(30), onPrimaryContainer: primary(90),
                secondaryContainer: secondary(30), onSecondaryContainer: secondary(90),
                tertiary: tertiary(80),
                error: error(80),
                surface: neutral(6),
                surfaceContainerLowest: neutral(4),
                surfaceContainerLow: neutral(10),
                surfaceContainer: neutral(12),
                surfaceContainerHigh: neutral(17),
                surfaceContainerHighest: neutral(22),
                onSurface: neutral(90),
                onSurfaceVariant: neutralVariant(80),
                outline: neutralVariant(60),
                outlineVariant: neutralVariant(30)
            )
        }
        return Md3Scheme(
            dark: false,
            primary: primary(40), onPrimary: primary(100),
            primaryContainer: primary(90), onPrimaryContainer: primary(10),
            secondaryContainer: secondary(90), onSecondaryContainer: secondary(10),
            tertiary: tertiary(40),
            error: error(40),
            surface: neutral(98),
            surfaceContainerLowest: neutral(100),
            surfaceContainerLow: neutral(96),
            surfaceContainer: neutral(94),
            surfaceContainerHigh: neutral(92),
            surfaceContainerHighest: neutral(90),
            onSurface: neutral(10),
            onSurfaceVariant: neutralVariant(30),
            outline: neutralVariant(50),
            outlineVariant: neutralVariant(80)
        )
    }
}

// MARK: - Canonical seeds (identical list to vdm-md3)

public let Md3Seeds: [(UInt32, String)] = [
    (0x4156A4, "Brand blue"),
    (0x6750A4, "Baseline purple"),
    (0x00639B, "Ocean"),
    (0x006A6A, "Teal"),
    (0x386A33, "Forest"),
    (0x8F4C00, "Sunset"),
    (0x984061, "Rose"),
    (0x56606F, "Slate"),
]

public let Md3DefaultSeed: UInt32 = 0x4156A4

public func md3SeedName(_ hex: UInt32) -> String {
    for (s, n) in Md3Seeds where s == hex { return n }
    return "Custom"
}

// MARK: - NSColor bridging

extension NSColor {
    static func md3(_ hex: UInt32, alpha: Double = 1.0) -> NSColor {
        NSColor(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255.0,
            green: CGFloat((hex >> 8) & 0xFF) / 255.0,
            blue: CGFloat(hex & 0xFF) / 255.0,
            alpha: CGFloat(alpha)
        )
    }
}

extension CGColor {
    static func md3(_ hex: UInt32, alpha: CGFloat = 1.0) -> CGColor {
        CGColor(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255.0,
            green: CGFloat((hex >> 8) & 0xFF) / 255.0,
            blue: CGFloat(hex & 0xFF) / 255.0,
            alpha: alpha
        )
    }
}
