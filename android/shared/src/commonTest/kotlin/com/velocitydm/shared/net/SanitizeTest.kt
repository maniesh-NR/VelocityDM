package com.velocitydm.shared.net

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SanitizeTest {

    @Test
    fun httpUrlValidation() {
        assertTrue(isHttpUrl("https://example.com/file.zip"))
        assertTrue(isHttpUrl("http://example.com"))
        assertTrue(isHttpUrl("  https://example.com  ")) // trimmed
        assertTrue(isHttpUrl("HTTP://EXAMPLE.COM")) // case-insensitive scheme
        // The old startsWith("http") bug class: look-alike schemes.
        assertFalse(isHttpUrl("httpx://example.com"))
        assertFalse(isHttpUrl("httpq://evil"))
        assertFalse(isHttpUrl("ftp://example.com"))
        assertFalse(isHttpUrl("magnet:?xt=urn:btih:x"))
        assertFalse(isHttpUrl("file:///etc/passwd"))
        assertFalse(isHttpUrl("javascript:alert(1)"))
        assertFalse(isHttpUrl("https://")) // no authority
        assertFalse(isHttpUrl("https://exa mple.com")) // whitespace inside
        assertFalse(isHttpUrl(""))
    }

    @Test
    fun filenameTraversalsAreNeutralized() {
        // Directory components are stripped; the final segment is kept
        // (that is the file the user actually asked for, and no
        // separator survives to traverse anything).
        assertEquals("passwd", sanitizeFilename("../../etc/passwd"))
        assertEquals("passwd", sanitizeFilename("..\\..\\etc\\passwd"))
        assertEquals("download", sanitizeFilename(".."))
        assertEquals("download", sanitizeFilename("."))
        assertEquals("download", sanitizeFilename(null))
        assertEquals("download", sanitizeFilename(""))
        assertEquals("download", sanitizeFilename("/"))
    }

    @Test
    fun urlDerivedNamesAreCleaned() {
        assertEquals("file.zip", filenameFromUrl("https://example.com/a/b/file.zip?x=1"))
        assertEquals("file.zip", filenameFromUrl("https://example.com/file.zip#frag"))
        assertEquals("download", filenameFromUrl("https://example.com/"))
        // no trailing slash: the host itself is the last segment
        assertEquals("example.com", filenameFromUrl("https://example.com"))
    }

    @Test
    fun controlCharactersAndLength() {
        assertEquals("ab.txt", sanitizeFilename("a\u0000b.txt")) // NUL removed
        assertEquals("line.txt", sanitizeFilename("line\n.txt"))
        assertEquals("ok.txt", sanitizeFilename("ok.txt"))
        val long = "x".repeat(500) + ".zip"
        assertTrue(sanitizeFilename(long).length <= 180)
    }

    @Test
    fun windowsReservedDeviceNames() {
        assertEquals("download", sanitizeFilename("CON"))
        assertEquals("download", sanitizeFilename("con.txt"))
        assertEquals("download", sanitizeFilename("NUL.bin"))
        assertEquals("download", sanitizeFilename("com1"))
        // "console.txt" is fine (not a bare device name)
        assertEquals("console.txt", sanitizeFilename("console.txt"))
    }

    @Test
    fun spacesAreKept() {
        assertEquals("my report.pdf", sanitizeFilename("my report.pdf"))
        assertEquals("my report.pdf", sanitizeFilename("/tmp/my report.pdf"))
    }
}
