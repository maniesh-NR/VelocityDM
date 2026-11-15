package com.velocitydm.shared.format

/** Human-readable byte/speed formatting (pure Kotlin, all targets). */

fun fmtBytes(b: Long): String {
    if (b < 1024) return "$b B"
    val units = listOf("KiB", "MiB", "GiB")
    var v = b.toDouble() / 1024
    var i = 0
    while (v >= 1024 && i < 2) {
        v /= 1024
        i++
    }
    // Locale-stable formatting (digits + '.' decimal separator, matching
    // the Rust engine's ASCII output and the desktop frontends).
    val s = v.toString()
    val dot = s.indexOf('.')
    val frac = if (dot >= 0) s.substring(dot + 1).take(1) else "0"
    val intPart = if (dot >= 0) s.substring(0, dot) else s
    return "$intPart.$frac ${units[i]}"
}

fun fmtSpeed(bps: Long): String = if (bps > 0) "${fmtBytes(bps)}/s" else "-"

/** Estimated time remaining, given bytes left and bytes/second. */
fun fmtEta(bytesLeft: Long, bps: Long): String {
    if (bps <= 0 || bytesLeft <= 0) return ""
    val secs = bytesLeft / bps
    return when {
        secs < 60 -> "${secs}s"
        secs < 3600 -> "${secs / 60}m ${secs % 60}s"
        else -> "${secs / 3600}h ${(secs % 3600) / 60}m"
    }
}

/**
 * Parse a human speed-limit expression like "2MB", "512k", "1.5GiB".
 * Returns 0 (no limit) when blank or unparseable. Decimal (k/m/g) and
 * binary (ki/mi/gi) prefixes both map to the 1024 family, matching the
 * engine's rate-limit units.
 */
fun parseLimit(s: String): Long {
    val t = s.trim().lowercase()
    if (t.isEmpty()) return 0
    val m = Regex("([0-9.]+)\\s*(k|m|g|ki|mi|gi)?b?").find(t) ?: return 0
    val n = m.groupValues[1].toDoubleOrNull() ?: return 0
    val mult = when (m.groupValues[2]) {
        "k", "ki" -> 1024.0
        "m", "mi" -> 1024.0 * 1024
        "g", "gi" -> 1024.0 * 1024 * 1024
        else -> 1.0
    }
    return (n * mult).toLong()
}
