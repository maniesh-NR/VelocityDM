package com.velocitydm.android.data

import android.app.Application
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.velocitydm.shared.engine.DownloadEngine
import com.velocitydm.shared.engine.DownloadOptions
import com.velocitydm.shared.engine.SegmentEngine
import com.velocitydm.shared.engine.TaskState
import com.velocitydm.shared.remote.RemoteEngine
import com.velocitydm.android.service.DownloadService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import java.io.File

/** Remote-mode surface state for the banner + settings UI. */
data class RemoteStatus(
    val active: Boolean,
    val stale: Boolean,
    val failure: String?,
)

/**
 * Downloads screen model (S12-e; S8-W1 adds the dual mode).
 *
 * LOCAL mode (default) is unchanged: the Kotlin SegmentEngine inside
 * this app. REMOTE mode makes the phone a remote control of a desktop
 * daemon over the authenticated LAN RPC (RemoteEngine). The two task
 * lists are NEVER merged — switching modes replaces the list wholesale
 * and pauses the side being left (local tasks stop gracefully and stay
 * resumable via .part/.vdmctl; the desktop daemon is untouched).
 */
class DownloadsViewModel(
    private val app: Application,
    private val engineScope: CoroutineScope,
) : ViewModel() {

    private val localEngine = SegmentEngine(engineScope)
    private var remoteEngine: RemoteEngine? = null
    private var mode: EngineMode = EngineMode.LOCAL

    private val _tasks = MutableStateFlow<List<TaskState>>(emptyList())
    val tasks: StateFlow<List<TaskState>> = _tasks

    private val _remoteStatus = MutableStateFlow(RemoteStatus(active = false, stale = false, failure = null))
    val remoteStatus: StateFlow<RemoteStatus> = _remoteStatus

    private val _downloadDir = MutableStateFlow(
        File(app.getExternalFilesDir(null) ?: app.filesDir, "downloads").apply { mkdirs() }
    )
    val downloadDir: StateFlow<File> = _downloadDir

    private val engine: DownloadEngine
        get() = remoteEngine ?: localEngine

    init {
        wire(localEngine, remote = false)
    }

    private fun wire(e: DownloadEngine, remote: Boolean) {
        e.listener = { t ->
            _tasks.value = e.tasks.toList()
            DownloadService.notify(app, t, remote = remote)
        }
    }

    /** Switch the engine (S8-W1 mode switch). Leaving local pauses all
     *  local tasks (graceful, resumable); leaving remote closes the
     *  stream (the daemon is untouched). Re-applying remote settings
     *  rebuilds the connection with the new values. */
    fun applyMode(settings: RemotePrefs.Settings) {
        val config = settings.toConfig()
        val newMode = if (settings.enabled && config != null) EngineMode.REMOTE else EngineMode.LOCAL
        if (newMode == EngineMode.LOCAL && mode == EngineMode.LOCAL) return
        when (mode) {
            EngineMode.LOCAL -> localEngine.stopAll()
            EngineMode.REMOTE -> {
                remoteEngine?.stop()
                remoteEngine = null
            }
        }
        mode = newMode
        if (newMode == EngineMode.REMOTE && config != null) {
            val remote = RemoteEngine(engineScope, config, RemotePrefs.tofuStore(app))
            remoteEngine = remote
            wire(remote, remote = true)
            remote.statusListener = { stale, failure ->
                _remoteStatus.value = RemoteStatus(active = true, stale = stale, failure = failure)
            }
            _remoteStatus.value = RemoteStatus(active = true, stale = false, failure = null)
            _tasks.value = emptyList() // wholesale replace, never merge
            remote.start()
        } else {
            _remoteStatus.value = RemoteStatus(active = false, stale = false, failure = null)
            _tasks.value = localEngine.tasks.toList()
        }
    }

    fun add(url: String, filename: String? = null, limitBps: Long = 0) {
        val current = engine
        engineScope.launch {
            if (current === localEngine) {
                DownloadService.ensureRunning(app)
            }
            current.add(
                url = url,
                dirPath = _downloadDir.value.absolutePath,
                opts = DownloadOptions(filename = filename, rateLimitBps = limitBps),
            )
            _tasks.value = current.tasks.toList()
        }
    }

    fun pause(id: Long) = engine.pause(id)

    fun resume(id: Long) {
        engineScope.launch { engine.resume(id) }
    }

    fun remove(id: Long) {
        engineScope.launch { engine.remove(id) }
    }

    fun setDir(dir: File) {
        _downloadDir.value = dir.apply { mkdirs() }
    }

    override fun onCleared() {
        remoteEngine?.stop()
        engineScope.coroutineContext[kotlinx.coroutines.Job]?.cancel()
        super.onCleared()
    }

    private enum class EngineMode { LOCAL, REMOTE }
}
