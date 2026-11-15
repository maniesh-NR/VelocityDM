package com.velocitydm.shared.engine

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class VdmctlTest {

    private fun sample() = VdmctlFile(
        url = "https://example.com/file.zip",
        size = 1000,
        etag = "\"abc\"",
        remaining = listOf(VdmctlInterval(0, 399), VdmctlInterval(600, 999)),
    )

    @Test
    fun roundTrip() {
        val f = sample()
        val text = Vdmctl.encode(f)
        val back = Vdmctl.parse(text)
        assertEquals(f, back)
    }

    @Test
    fun rejectsUnknownKeys() {
        val f = sample()
        val text = Vdmctl.encode(f)
        // unknown key -> rejected (strict schema)
        val tampered = """
            {"url":"https://example.com/file.zip","size":1000,"etag":"x",
             "remaining":[{"start":0,"end":10}],"injected":true}
        """.trimIndent()
        assertNull(Vdmctl.parse(tampered))
        assertNull(Vdmctl.parse("{}"))
        assertNull(Vdmctl.parse("not json"))
        assertEquals(f, Vdmctl.parse(text)) // sanity: the untouched encode still parses
    }

    @Test
    fun rejectsOutOfRangeIntervals() {
        // start < 0
        assertNull(Vdmctl.parse(encode(-1, 10, size = 100)))
        // end >= size
        assertNull(Vdmctl.parse(encode(0, 100, size = 100)))
        // end < start
        assertNull(Vdmctl.parse(encode(10, 5, size = 100)))
        // size <= 0
        assertNull(Vdmctl.parse(encode(0, 10, size = 0)))
        // empty remaining
        assertNull(Vdmctl.parse(encode(0, -1, size = 100)))
    }

    @Test
    fun rejectsCoverageOverflow() {
        // Two disjoint intervals claiming more than the file size.
        val text = """
            {"url":"u","size":100,"etag":"",
             "remaining":[{"start":0,"end":59},{"start":50,"end":99}]}
        """.trimIndent()
        assertNull(Vdmctl.parse(text))
    }

    @Test
    fun resumePolicy() {
        val stored = sample() // etag "abc", size 1000
        // matching etag -> resumable
        assertTrue(Vdmctl.resumableWith(stored, 1000, "\"abc\""))
        // etag mismatch -> restart
        assertFalse(Vdmctl.resumableWith(stored, 1000, "\"xyz\""))
        // server sent no etag: only an exact size match resumes
        assertTrue(Vdmctl.resumableWith(stored, 1000, null))
        assertFalse(Vdmctl.resumableWith(stored, 2000, null))
        // blank etag from server behaves like absent
        assertTrue(Vdmctl.resumableWith(stored, 1000, ""))
        // stored size mismatch -> restart regardless
        assertFalse(Vdmctl.resumableWith(stored, 2000, "\"abc\""))
        // null/invalid stored -> restart
        assertFalse(Vdmctl.resumableWith(null, 1000, "\"abc\""))
        assertFalse(Vdmctl.resumableWith(VdmctlFile("u", -1), 1000, "\"abc\""))
    }

    @Test
    fun parseReturnsValidatedFile() {
        assertNotNull(Vdmctl.parse(Vdmctl.encode(sample())))
        // isValid exposed for callers holding deserialized values
        assertTrue(Vdmctl.isValid(sample()))
        assertFalse(Vdmctl.isValid(sample().copy(size = 5)))
    }

    private fun encode(start: Long, end: Long, size: Long): String =
        Vdmctl.encode(
            VdmctlFile(
                url = "u",
                size = size,
                remaining = if (end >= start) listOf(VdmctlInterval(start, end)) else emptyList(),
            ),
        )
}
