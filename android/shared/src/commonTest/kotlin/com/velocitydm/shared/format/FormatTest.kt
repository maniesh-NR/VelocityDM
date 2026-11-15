package com.velocitydm.shared.format

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class FormatTest {

    @Test
    fun fmtBytes() {
        assertEquals("512 B", fmtBytes(512))
        assertEquals("1.0 KiB", fmtBytes(1024))
        assertEquals("1.5 KiB", fmtBytes(1536))
        assertEquals("2.0 MiB", fmtBytes(2L * 1024 * 1024))
        assertEquals("1.0 GiB", fmtBytes(1L * 1024 * 1024 * 1024))
        // truncation to one decimal, never rounding up a unit
        assertEquals("1023.9 KiB", fmtBytes(1024L * 1024 - 1))
    }

    @Test
    fun fmtSpeed() {
        assertEquals("-", fmtSpeed(0))
        assertEquals("-", fmtSpeed(-5))
        assertEquals("1.0 KiB/s", fmtSpeed(1024))
    }

    @Test
    fun fmtEta() {
        assertEquals("", fmtEta(1000, 0))
        assertEquals("", fmtEta(0, 100))
        assertEquals("45s", fmtEta(4500, 100))
        assertEquals("2m 5s", fmtEta(125_000, 1000))
        assertEquals("1h 1m", fmtEta(3_660_000, 1000))
    }

    @Test
    fun parseLimitVariants() {
        assertEquals(0, parseLimit(""))
        assertEquals(0, parseLimit("   "))
        assertEquals(0, parseLimit("abc"))
        assertEquals(2048L * 1024, parseLimit("2MB"))
        assertEquals(512L * 1024, parseLimit("512k"))
        assertEquals(512L * 1024, parseLimit("512KB"))
        assertEquals((1.5 * 1024 * 1024).toLong(), parseLimit("1.5MiB"))
        assertEquals(1024L * 1024 * 1024, parseLimit("1GiB"))
        assertEquals(100, parseLimit("100"))
        assertTrue(parseLimit("2 mb") > 0)
    }
}
