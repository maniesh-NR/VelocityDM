package com.velocitydm.app.ui

// SPDX-License-Identifier: GPL-3.0-or-later

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.velocitydm.app.data.SettingsRepository
import com.velocitydm.app.data.db.AppDatabase
import com.velocitydm.app.data.db.DownloadEntity
import com.velocitydm.app.engine.UriUtils
import com.velocitydm.app.service.DownloadService
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

data class DownloadUiItem(
    val id: Long,
    val url: String,
    val filename: String,
    val state: String,
    val downloaded: Long,
    val total: Long,
    val errorMsg: String?,
)

data class AppUiSettings(
    val downloadDir: String = "",
    val defaultParts: Int = 8,
    val speedLimitBps: Long = 0,
    val refreshHz: Int = 12,
    val themeMode: String = com.velocitydm.app.data.AppSettings.THEME_SYSTEM,
)

/**
 * UI state holder. Download rows come from Room; live progress arrives
 * through the service's throttled snapshot flow (10..30 Hz cap), so the UI
 * never observes raw engine internals.
 */
class DownloadsViewModel(app: Application) : AndroidViewModel(app) {

    private val dao = AppDatabase.get(app).downloadDao()
    private val settings = SettingsRepository(app)

    val downloads: StateFlow<List<DownloadUiItem>> = dao.observeAll().map { rows ->
        rows.map { row ->
            DownloadUiItem(
                id = row.id,
                url = row.url,
                filename = row.filename,
                state = row.state,
                downloaded = row.downloaded,
                total = row.total,
                errorMsg = row.errorMsg,
            )
        }
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), emptyList())

    val settingsUi: StateFlow<AppUiSettings> = settings.settings.map { s ->
        AppUiSettings(s.downloadDir, s.defaultParts, s.speedLimitBps, s.refreshHz, s.themeMode)
    }.stateIn(viewModelScope, SharingStarted.Eagerly, AppUiSettings())

    private val _liveProgress = MutableStateFlow<Map<Long, Pair<Long, Long>>>(emptyMap())
    val liveProgress: StateFlow<Map<Long, Pair<Long, Long>>> = _liveProgress

    init {
        viewModelScope.launch {
            DownloadService.progressFlow(getApplication()).collect { snap ->
                _liveProgress.value = _liveProgress.value + (snap.taskId to (snap.downloaded to snap.total))
            }
        }
    }

    /** Validates and queues a URL; returns an error message or null. */
    fun enqueue(url: String, filename: String?, checksum: String?, wifiOnly: Boolean, onQueued: (String?) -> Unit) {
        val trimmed = url.trim()
        if (!trimmed.startsWith("http://") && !trimmed.startsWith("https://")) {
            onQueued("Only http and https URLs can be queued.")
            return
        }
        val derived = filename?.trim()?.takeIf { it.isNotEmpty() }
            ?: UriUtils.filenameFromUrl(trimmed)
            ?: "download-${System.currentTimeMillis()}.bin"
        viewModelScope.launch {
            val dir = settingsUi.value.downloadDir
            val id = dao.insert(
                DownloadEntity(
                    url = trimmed,
                    filename = derived,
                    dir = dir,
                    checksum = checksum?.trim()?.takeIf { it.isNotEmpty() },
                    wifiOnly = wifiOnly,
                ),
            )
            DownloadService.enqueueId(getApplication(), id)
            onQueued(null)
        }
    }

    fun pause(id: Long) = viewModelScope.launch {
        DownloadService.sendAction(getApplication(), DownloadService.ACTION_PAUSE, id)
    }

    fun resume(id: Long) = viewModelScope.launch {
        DownloadService.sendAction(getApplication(), DownloadService.ACTION_RESUME, id)
    }

    fun remove(id: Long) = viewModelScope.launch {
        DownloadService.sendAction(getApplication(), DownloadService.ACTION_CANCEL, id)
    }

    fun setDefaultParts(value: Int) = viewModelScope.launch { settings.setDefaultParts(value) }

    fun setSpeedLimit(bps: Long) = viewModelScope.launch { settings.setSpeedLimitBps(bps) }

    fun setRefreshHz(hz: Int) = viewModelScope.launch { settings.setRefreshHz(hz) }

    fun setThemeMode(mode: String) = viewModelScope.launch { settings.setThemeMode(mode) }
}
