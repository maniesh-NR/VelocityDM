package com.velocitydm.app.util

// SPDX-License-Identifier: GPL-3.0-or-later

import java.util.Locale

/** Human-readable formatters for sizes, speeds, and ETAs. */
object Formatters {

    fun formatBytes(bytes: Long): String {
        if (bytes < 0) return "?"
        var value = bytes.toDouble()
        var unit = 0
        while (value >= 1024.0 && unit < UNITS.size - 1) {
            value /= 1024.0
            unit += 1
        }
        return if (unit == 0) {
            String.format(Locale.US, "%d B", bytes)
        } else {
            String.format(Locale.US, "%.1f %s", value, UNITS[unit])
        }
    }

    fun formatSpeed(bytesPerSec: Long): String =
        if (bytesPerSec <= 0) "" else formatBytes(bytesPerSec) + "/s"

    fun formatEta(seconds: Long): String {
        if (seconds <= 0 || seconds > 86_400) return ""
        val h = seconds / 3600
        val m = (seconds % 3600) / 60
        val s = seconds % 60
        return when {
            h > 0 -> String.format(Locale.US, "%dh%02dm", h, m)
            m > 0 -> String.format(Locale.US, "%dm%02ds", m, s)
            else -> String.format(Locale.US, "%ds", s)
        }
    }

    private val UNITS = arrayOf("B", "KB", "MB", "GB", "TB")
}
