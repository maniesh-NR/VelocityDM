package com.velocitydm.shared.md3

import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.cbrt
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * VelocityDM shared Material You (MD3) dynamic-color token engine —
 * Kotlin port of the canonical Rust crate `vdm-md3`.
 *
 * Identical OKLab <-> sRGB math (Björn Ottosson's public-domain
 * formulation), identical tone tables, full 27-field scheme — so one
 * seed color renders the same roles on Android as on every other
 * platform. The Rust crate remains canonical; drift between the two is
 * tripped by `Md3GoldenTest`, which asserts every value of every named
 * seed against `md3-tokens-baseline.json` generated from the Rust
 * engine (scripts/gen-md3-baseline.py). After any deliberate color-math
 * change, regenerate the baseline and update this port in the same
 * commit.
 *
 * Clean-room: no Google source code, tables, or resources are used.
 */

// ---------------------------------------------------------------------------
// OKLab / OKLCH <-> sRGB (public-domain formulation)
// ---------------------------------------------------------------------------

private fun srgbToLinear(c: Double): Double =
    if (c <= 0.04045) c / 12.92 else ((c + 0.055) / 1.055).pow(2.4)

private fun linearToSrgb(c: Double): Double =
    if (c <= 0.0031308) c * 12.92 else 1.055 * c.pow(1.0 / 2.4) - 0.055

private fun linearToOklab(r: Double, g: Double, b: Double): Triple<Double, Double, Double> {
    val l = 0.4122214708 * r + 0.536332512 * g + 0.0514459929 * b
    val m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    val s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    val l_ = cbrt(l)
    val m_ = cbrt(m)
    val s_ = cbrt(s)
    return Triple(
        0.2104542553 * l_ + 0.793617785 * m_ - 0.0040720468 * s_,
        1.9779984954 * l_ - 2.428592205 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.808675766 * s_,
    )
}

private fun oklabToLinear(l: Double, a: Double, b: Double): Triple<Double, Double, Double> {
    val l_ = l + 0.3963377774 * a + 0.2158037573 * b
    val m_ = l - 0.1055613458 * a - 0.0638541728 * b
    val s_ = l - 0.0894841775 * a - 1.291485548 * b
    val l3 = l_ * l_ * l_
    val m3 = m_ * m_ * m_
    val s3 = s_ * s_ * s_
    return Triple(
        4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3,
        -1.2684386269 * l3 + 2.6097573729 * m3 + 0.3413193877 * s3,
        -0.0041960865 * l3 - 0.7034186147 * m3 + 1.7076146937 * s3,
    )
}

private fun clamp01(x: Double): Double = x.coerceIn(0.0, 1.0)

private fun inGamut(r: Double, g: Double, b: Double): Boolean =
    r >= -1e-4 && r <= 1.0 + 1e-4 &&
        g >= -1e-4 && g <= 1.0 + 1e-4 &&
        b >= -1e-4 && b <= 1.0 + 1e-4

/**
 * One OKLCH point (tone = L in percent, chroma, hue degrees) as packed
 * `0xRRGGBB`, chroma reduced as needed to stay in the sRGB gamut
 * (binary search, 24 iterations).
 */
private fun oklchToHex(tone: Double, chromaIn: Double, hueDeg: Double): Int {
    val rad = Math.toRadians(hueDeg)
    var c = chromaIn
    val out = oklabToLinear(tone / 100.0, c * cos(rad), c * sin(rad))
    if (!inGamut(out.first, out.second, out.third)) {
        var lo = 0.0
        var hi = c
        repeat(24) {
            val mid = (lo + hi) / 2.0
            val t = oklabToLinear(tone / 100.0, mid * cos(rad), mid * sin(rad))
            if (inGamut(t.first, t.second, t.third)) lo = mid else hi = mid
        }
        c = lo
    }
    val fin = oklabToLinear(tone / 100.0, c * cos(rad), c * sin(rad))
    val rr = (linearToSrgb(clamp01(fin.first)) * 255.0).roundToInt()
    val gg = (linearToSrgb(clamp01(fin.second)) * 255.0).roundToInt()
    val bb = (linearToSrgb(clamp01(fin.third)) * 255.0).roundToInt()
    return (rr shl 16) or (gg shl 8) or bb
}

/** sRGB bytes -> OKLCH (L in 0..1, chroma, hue degrees in 0..360). */
internal fun rgbToOklch(r: Int, g: Int, b: Int): Triple<Double, Double, Double> {
    val lr = srgbToLinear(r / 255.0)
    val lg = srgbToLinear(g / 255.0)
    val lb = srgbToLinear(b / 255.0)
    val (l, a, bb) = linearToOklab(lr, lg, lb)
    val chroma = sqrt(a * a + bb * bb)
    var hue = Math.toDegrees(atan2(bb, a))
    if (hue < 0.0) hue += 360.0
    return Triple(l, chroma, hue)
}

// ---------------------------------------------------------------------------
// Tonal palettes
// ---------------------------------------------------------------------------

/** A tonal palette: fixed hue + chroma, sampled at tones 0..100. */
class Palette(private val hue: Double, private val chroma: Double) {
    /** Tone 0..100 -> packed `0xRRGGBB` (tone == OKLab L in percent). */
    fun tone(t: Double): Int = oklchToHex(t, chroma, hue)
}

// ---------------------------------------------------------------------------
// Scheme (MD3 color roles) — the full 27-field Rust role set
// ---------------------------------------------------------------------------

/**
 * Full Material Design 3 color-role set generated from one seed color,
 * every field a packed `0xRRGGBB` value. Serializable so the golden
 * test can consume the Rust-generated baseline JSON directly (the
 * camelCase field names match the baseline keys).
 */
@kotlinx.serialization.Serializable
data class Md3Scheme(
    val dark: Boolean,
    val primary: Int,
    val onPrimary: Int,
    val primaryContainer: Int,
    val onPrimaryContainer: Int,
    val secondaryContainer: Int,
    val onSecondaryContainer: Int,
    val tertiary: Int,
    val onTertiaryContainer: Int,
    val tertiaryContainer: Int,
    val error: Int,
    val onError: Int,
    val errorContainer: Int,
    val onErrorContainer: Int,
    val surface: Int,
    val surfaceContainerLowest: Int,
    val surfaceContainerLow: Int,
    val surfaceContainer: Int,
    val surfaceContainerHigh: Int,
    val surfaceContainerHighest: Int,
    val onSurface: Int,
    val onSurfaceVariant: Int,
    val outline: Int,
    val outlineVariant: Int,
    val inverseSurface: Int,
    val inverseOnSurface: Int,
    val inversePrimary: Int,
    val scrim: Int,
) {
    companion object {
        /** Generate the light or dark scheme from sRGB bytes. */
        fun fromSeedRgb(r: Int, g: Int, b: Int, dark: Boolean): Md3Scheme {
            val (_, c, h) = rgbToOklch(r, g, b)
            // Chroma boost: a dull seed still yields a vivid accent, as
            // in Material You's "source color" processing.
            val boost = maxOf(c, if (dark) 0.115 else 0.13)
            val primary = Palette(h, boost)
            val secondary = Palette(h, maxOf(boost * 0.34, 0.028))
            val tertiary = Palette((h + 52.0) % 360.0, maxOf(boost * 0.72, 0.075))
            val neutral = Palette(h, 0.006)
            val neutralVariant = Palette(h, 0.014)
            val error = Palette(27.0, 0.155)

            return if (dark) {
                Md3Scheme(
                    dark = true,
                    primary = primary.tone(80.0),
                    onPrimary = primary.tone(20.0),
                    primaryContainer = primary.tone(30.0),
                    onPrimaryContainer = primary.tone(90.0),
                    secondaryContainer = secondary.tone(30.0),
                    onSecondaryContainer = secondary.tone(90.0),
                    tertiary = tertiary.tone(80.0),
                    onTertiaryContainer = tertiary.tone(90.0),
                    tertiaryContainer = tertiary.tone(30.0),
                    error = error.tone(80.0),
                    onError = error.tone(20.0),
                    errorContainer = error.tone(30.0),
                    onErrorContainer = error.tone(90.0),
                    surface = neutral.tone(6.0),
                    surfaceContainerLowest = neutral.tone(4.0),
                    surfaceContainerLow = neutral.tone(10.0),
                    surfaceContainer = neutral.tone(12.0),
                    surfaceContainerHigh = neutral.tone(17.0),
                    surfaceContainerHighest = neutral.tone(22.0),
                    onSurface = neutral.tone(90.0),
                    onSurfaceVariant = neutralVariant.tone(80.0),
                    outline = neutralVariant.tone(60.0),
                    outlineVariant = neutralVariant.tone(30.0),
                    inverseSurface = neutral.tone(90.0),
                    inverseOnSurface = neutral.tone(20.0),
                    inversePrimary = primary.tone(40.0),
                    scrim = neutral.tone(0.0),
                )
            } else {
                Md3Scheme(
                    dark = false,
                    primary = primary.tone(40.0),
                    onPrimary = primary.tone(100.0),
                    primaryContainer = primary.tone(90.0),
                    onPrimaryContainer = primary.tone(10.0),
                    secondaryContainer = secondary.tone(90.0),
                    onSecondaryContainer = secondary.tone(10.0),
                    tertiary = tertiary.tone(40.0),
                    onTertiaryContainer = tertiary.tone(10.0),
                    tertiaryContainer = tertiary.tone(90.0),
                    error = error.tone(40.0),
                    onError = error.tone(100.0),
                    errorContainer = error.tone(90.0),
                    onErrorContainer = error.tone(10.0),
                    surface = neutral.tone(98.0),
                    surfaceContainerLowest = neutral.tone(100.0),
                    surfaceContainerLow = neutral.tone(96.0),
                    surfaceContainer = neutral.tone(94.0),
                    surfaceContainerHigh = neutral.tone(92.0),
                    surfaceContainerHighest = neutral.tone(90.0),
                    onSurface = neutral.tone(10.0),
                    onSurfaceVariant = neutralVariant.tone(30.0),
                    outline = neutralVariant.tone(50.0),
                    outlineVariant = neutralVariant.tone(80.0),
                    inverseSurface = neutral.tone(20.0),
                    inverseOnSurface = neutral.tone(95.0),
                    inversePrimary = primary.tone(80.0),
                    scrim = neutralVariant.tone(0.0),
                )
            }
        }

        /** Build with an explicit hex seed (`0xRRGGBB`). */
        fun fromSeedHex(seedHex: Int, dark: Boolean): Md3Scheme =
            fromSeedRgb((seedHex shr 16) and 0xFF, (seedHex shr 8) and 0xFF, seedHex and 0xFF, dark)

        /** Parse `#RRGGBB` or `RRGGBB`; `null` when malformed. */
        fun parseHex(s: String): Int? {
            val h = s.trim().removePrefix("#")
            if (h.length != 6 || !h.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }) return null
            return h.toIntOrNull(16)
        }
    }
}

// ---------------------------------------------------------------------------
// Canonical seed choices (identical list to vdm-md3)
// ---------------------------------------------------------------------------

/** Named seed colors shown in every frontend's Material You picker. */
val MD3_SEEDS: List<Pair<Int, String>> = listOf(
    0x4156A4 to "Brand blue",
    0x6750A4 to "Baseline purple",
    0x00639B to "Ocean",
    0x006A6A to "Teal",
    0x386A33 to "Forest",
    0x8F4C00 to "Sunset",
    0x984061 to "Rose",
    0x56606F to "Slate",
)

/** Default seed: the brand blue used across VelocityDM's surfaces. */
const val MD3_DEFAULT_SEED: Int = 0x4156A4

fun md3SeedName(seedHex: Int): String =
    MD3_SEEDS.firstOrNull { it.first == seedHex }?.second ?: "Custom"
