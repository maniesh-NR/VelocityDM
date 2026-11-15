package com.velocitydm.shared.md3

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class Md3Test {

    @Test
    fun parseHexVariants() {
        assertEquals(0x4156A4, Md3Scheme.parseHex("#4156A4"))
        assertEquals(0x4156A4, Md3Scheme.parseHex("4156a4"))
        assertNull(Md3Scheme.parseHex("#12345"))
        assertNull(Md3Scheme.parseHex("zzzzzz"))
    }

    @Test
    fun tonesAreMonotonicInLightness() {
        // Neutral palette: tone 6 (dark surface) must be far darker than
        // tone 90 (dark on-surface) in perceived lightness.
        val p = Palette(264.0, 0.006)
        fun lOf(hex: Int): Double {
            val (l, _, _) = rgbToOklch(
                (hex shr 16) and 0xFF,
                (hex shr 8) and 0xFF,
                hex and 0xFF,
            )
            return l
        }
        val l6 = lOf(p.tone(6.0))
        val l90 = lOf(p.tone(90.0))
        assertTrue(l6 < 0.2, "tone 6 should be dark, got L=$l6")
        assertTrue(l90 > 0.8, "tone 90 should be light, got L=$l90")
    }

    @Test
    fun darkAndLightSchemesDifferInEverySurfaceRole() {
        val d = Md3Scheme.fromSeedHex(0x4156A4, dark = true)
        val l = Md3Scheme.fromSeedHex(0x4156A4, dark = false)
        assertNotEquals(d.surface, l.surface)
        assertNotEquals(d.primary, l.primary)
        assertNotEquals(d.onPrimary, l.onPrimary)
        assertNotEquals(d.outline, l.outline)
        assertTrue(d.dark)
        assertTrue(!l.dark)
    }

    @Test
    fun schemeRolesAreInGamutBytes() {
        for (dark in listOf(true, false)) {
            val s = Md3Scheme.fromSeedHex(0x8F4C00, dark)
            for (v in listOf(s.primary, s.onPrimary, s.surface, s.onSurface, s.error, s.outline)) {
                assertEquals(0, v and (0xFF shl 24), "role value must be 24-bit: ${v.toString(16)}")
            }
        }
    }

    @Test
    fun deterministicAcrossCalls() {
        val a = Md3Scheme.fromSeedHex(MD3_DEFAULT_SEED, true)
        val b = Md3Scheme.fromSeedHex(MD3_DEFAULT_SEED, true)
        assertEquals(a, b)
    }

    @Test
    fun dullSeedStillGetsVividAccent() {
        // Chroma boost: a gray seed still yields a colored primary.
        val dull = Md3Scheme.fromSeedRgb(0x80, 0x80, 0x80, dark = false).primary
        val (_, chroma, _) = rgbToOklch(
            (dull shr 16) and 0xFF,
            (dull shr 8) and 0xFF,
            dull and 0xFF,
        )
        assertTrue(chroma >= 0.09, "dull seed chroma boost failed: $chroma")
    }

    @Test
    fun seedListMatchesCanonicalSet() {
        assertEquals(8, MD3_SEEDS.size)
        assertEquals(0x4156A4 to "Brand blue", MD3_SEEDS[0])
        assertEquals("Custom", md3SeedName(0x123456))
        assertEquals("Ocean", md3SeedName(0x00639B))
    }
}
