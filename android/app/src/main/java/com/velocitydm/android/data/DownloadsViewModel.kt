package com.velocitydm.android.data

import android.app.Application
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.velocitydm.android.engine.DownloadOptions
import com.velocitydm.android.engine.SegmentEngine
import com.velocitydm.android.engine.TaskState
import com.velocitydm.android.service.DownloadService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import java.io.File

class DownloadsViewModel(
    private val app: Application,
    private val engineScope: CoroutineScope,
) : ViewModel() {

    private val engine = SegmentEngine(engineScope)
    private val _tasks = MutableStateFlow<List<TaskState>>(emptyList())
    val tasks: StateFlow<List<TaskState>> = _tasks

    private val _downloadDir = MutableStateFlow(
        File(app.getExternalFilesDir(null) ?: app.filesDir, "downloads").apply { mkdirs() }
    )
    val downloadDir: StateFlow<File> = _downloadDir

    init {
        engine.listener = { t ->
            _tasks.value = engine.tasks.toList()
            DownloadService.notify(app, t)
        }
    }

    fun add(url: String, filename: String? = null, limitBps: Long = 0) {
        engineScope.launch {
            DownloadService.ensureRunning(app)
            engine.add(
                url = url,
                dir = _downloadDir.value,
                opts = DownloadOptions(filename = filename, rateLimitBps = limitBps),
            )
            _tasks.value = engine.tasks.toList()
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
        engineScope.coroutineContext[ kotlinx.coroutines.Job]?.cancel()
        super.onCleared()
    }
}
