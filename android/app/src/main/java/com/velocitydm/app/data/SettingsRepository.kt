package com.velocitydm.app.data

// SPDX-License-Identifier: GPL-3.0-or-later

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "velocitydm_settings")

data class AppSettings(
    val downloadDir: String = "",
    val defaultParts: Int = 8,
    val speedLimitBps: Long = 0,
    val refreshHz: Int = 12,
    val themeMode: String = THEME_SYSTEM,
) {
    companion object {
        const val THEME_SYSTEM = "system"
        const val THEME_LIGHT = "light"
        const val THEME_DARK = "dark"
    }
}

/** DataStore-backed app settings; engine and UI both read through this. */
class SettingsRepository(private val context: Context) {

    private object Keys {
        val downloadDir = stringPreferencesKey("download_dir")
        val defaultParts = intPreferencesKey("default_parts")
        val speedLimitBps = longPreferencesKey("speed_limit_bps")
        val refreshHz = intPreferencesKey("refresh_hz")
        val themeMode = stringPreferencesKey("theme_mode")
    }

    val settings: Flow<AppSettings> = context.dataStore.data.map { prefs ->
        AppSettings(
            downloadDir = prefs[Keys.downloadDir] ?: defaultDownloadDir(),
            defaultParts = (prefs[Keys.defaultParts] ?: 8).coerceIn(1, 16),
            speedLimitBps = prefs[Keys.speedLimitBps] ?: 0L,
            // The UI refresh rate is capped to 10..30 Hz by design.
            refreshHz = (prefs[Keys.refreshHz] ?: 12).coerceIn(10, 30),
            themeMode = prefs[Keys.themeMode] ?: AppSettings.THEME_SYSTEM,
        )
    }

    suspend fun setDownloadDir(value: String) = context.dataStore.edit { it[Keys.downloadDir] = value }
    suspend fun setDefaultParts(value: Int) = context.dataStore.edit { it[Keys.defaultParts] = value.coerceIn(1, 16) }
    suspend fun setSpeedLimitBps(value: Long) = context.dataStore.edit { it[Keys.speedLimitBps] = value.coerceAtLeast(0) }
    suspend fun setRefreshHz(value: Int) = context.dataStore.edit { it[Keys.refreshHz] = value.coerceIn(10, 30) }
    suspend fun setThemeMode(value: String) = context.dataStore.edit { it[Keys.themeMode] = value }

    private fun defaultDownloadDir(): String =
        java.io.File(
            android.os.Environment.getExternalStoragePublicDirectory(android.os.Environment.DIRECTORY_DOWNLOADS),
            "VelocityDM",
        ).absolutePath
}
