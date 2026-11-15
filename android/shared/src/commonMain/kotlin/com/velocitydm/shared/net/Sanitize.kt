package com.velocitydm.shared.net

/**
 * URL / filename hygiene shared by every download-entry path (add
 * dialog, VIEW-intent auto-add, browser sniffing). Gate 0005 hardened
 * these: the old `url.startsWith("http")` check accepted schemes like
 * `httpx://`, and the old `substringAfterLast('/')` filename
 * derivation let path separators and `..` through into `File(dir, name)`.
 */

/** True only for well-formed http/https URLs. */
fun isHttpUrl(s: String): Boolean {
    val t = s.trim()
    if (!t.startsWith("http://", ignoreCase = true) &&
        !t.startsWith("https://", ignoreCase = true)
    ) {
        return false
    }
    // Scheme separator must be followed by a plausible authority.
    val rest = t.substringAfter("://", missingDelimiterValue = "")
    if (rest.isBlank()) return false
    val bad = rest.indexOfFirst { it == ' ' || it == '\t' || it == '\n' || it == '\r' }
    return bad < 0
}

/**
 * Derive a safe filename from a URL's last path segment (or a
 * user-supplied override). Rejects path separators, `..`, absolute
 * paths, and control characters; caps the length; falls back to
 * "download" when nothing safe remains.
 */
fun sanitizeFilename(raw: String?): String {
    if (raw == null) return "download"
    // Strip any directory component a caller may have smuggled in.
    var name = raw.substringAfterLast('/').substringAfterLast('\\')
    // Drop query/fragment remnants from URL-derived names.
    name = name.substringBefore('?').substringBefore('#')
    // Drop control characters; spaces are legal and stay.
    name = name.filter { it.code >= 0x20 }.trim()
    if (name.isEmpty() || name == "." || name == "..") return "download"
    // Windows-reserved device names make bad files everywhere; be safe.
    val lower = name.lowercase()
    val reserved = listOf(
        "con", "prn", "aux", "nul",
        "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
        "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    )
    if (lower.substringBefore('.') in reserved) return "download"
    if (name.length > 180) name = name.take(180)
    return name.ifBlank { "download" }
}

/** URL last-segment name (pre-sanitization), e.g. `a/b/file.zip?x=1` -> `file.zip`. */
fun filenameFromUrl(url: String): String =
    sanitizeFilename(url.substringAfterLast('/'))
