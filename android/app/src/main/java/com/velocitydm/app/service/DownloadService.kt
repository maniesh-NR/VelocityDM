package com.velocitydm.app.service

// SPDX-License-Identifier: GPL-3.0-or-later

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import com.velocitydm.app.MainActivity
import com.velocitydm.app.R
import com.velocitydm.app.data.db.AppDatabase
import com.velocitydm.app.data.db.DownloadEntity
import com.velocitydm.app.engine.DownloadEngine
import com.velocitydm.app.engine.EngineResult
import com.velocitydm.app.engine.EngineSpec
import com.velocitydm.app.engine.HttpRangeClient
import com.velocitydm.app.engine.ProgressThrottler
import com.velocitydm.app.engine.RateLimiter
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean

/** Foreground service that runs up to [MAX_CONCURRENT] download engines. */
class DownloadService : Service() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val jobs = ConcurrentHashMap<Long, Job>()
    private val pauseFlags = ConcurrentHashMap<Long, AtomicBoolean>()
    private lateinit var notifier: Notifier
    private var emitter: Job? = null

    override fun onCreate() {
        super.onCreate()
        notifier = Notifier(this)
        startForeground(NOTIFICATION_ID, notifier.buildSummary(getString(R.string.service_running)))
        // Bridge engine throttler snapshots into the static flow the UI
        // collects (10..30 Hz, never faster).
        emitter = scope.launch {
            engineProgress.collect { snap -> progressFlow.emit(snap) }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_PAUSE -> intent.getLongExtra(EXTRA_ID, -1).takeIf { it >= 0 }?.let { pause(it) }
            ACTION_RESUME -> intent.getLongExtra(EXTRA_ID, -1).takeIf { it >= 0 }?.let { resume(it) }
            ACTION_CANCEL -> intent.getLongExtra(EXTRA_ID, -1).takeIf { it >= 0 }?.let { cancel(it) }
            ACTION_ENQUEUE -> pumpQueue()
            else -> pumpQueue()
        }
        return START_STICKY
    }

    private fun pumpQueue() {
        scope.launch {
            val dao = AppDatabase.get(this@DownloadService).downloadDao()
            val settings = SettingsBridge.settings(this@DownloadService).first()
            val running = dao.byState(DownloadEntity.STATE_RUNNING)
            val queued = dao.byState(DownloadEntity.STATE_QUEUED)
            val free = (MAX_CONCURRENT - running.size).coerceAtLeast(0)
            for (entity in queued.take(free)) {
                startEngine(entity, settings)
            }
            if (jobs.isEmpty() && queued.isEmpty()) {
                stopForeground(STOP_FOREGROUND_DETACH)
            }
        }
    }

    private suspend fun startEngine(entity: DownloadEntity, settings: AppSettingsSnapshot) {
        if (jobs.containsKey(entity.id)) return
        daoSetState(entity.id, DownloadEntity.STATE_RUNNING)
        val flag = AtomicBoolean(false)
        pauseFlags[entity.id] = flag
        val throttleScope = CoroutineScope(scope.coroutineContext)
        val throttler = ProgressThrottler(throttleScope, settings.refreshHz)
        val limiter = RateLimiter(settings.speedLimitBps)
        val spec = EngineSpec(
            url = entity.url,
            mirrors = parseMirrors(entity.mirrorsJson),
            filename = entity.filename,
            dir = File(entity.dir),
            parts = settings.defaultParts,
            rateLimitBps = settings.speedLimitBps,
            checksum = entity.checksum?.let { parseChecksum(it) },
            wifiOnly = entity.wifiOnly,
        )
        val engine = DownloadEngine(spec, HttpRangeClient(), limiter, throttler, flag)
        engineProgressBridge = throttler
        val dao = AppDatabase.get(this).downloadDao()
        jobs[entity.id] = scope.launch {
            notifier.progress(entity.id, entity.filename)
            when (val result = engine.run()) {
                is EngineResult.Success -> {
                    dao.setProgress(entity.id, result.bytes, result.bytes)
                    dao.setCompleted(entity.id, DownloadEntity.STATE_COMPLETED, System.currentTimeMillis())
                    notifier.completed(entity.id, entity.filename)
                }
                is EngineResult.Failure -> {
                    if (result.error == "paused") {
                        dao.setState(entity.id, DownloadEntity.STATE_PAUSED)
                    } else {
                        dao.setError(entity.id, DownloadEntity.STATE_FAILED, result.error)
                        notifier.failed(entity.id, entity.filename, result.error)
                    }
                }
            }
            jobs.remove(entity.id)
            pauseFlags.remove(entity.id)
            pumpQueue()
        }
    }

    private fun pause(id: Long) {
        pauseFlags[id]?.set(true)
        scope.launch { AppDatabase.get(this@DownloadService).downloadDao().setState(id, DownloadEntity.STATE_PAUSED) }
        notifier.updateText(id, "Paused")
    }

    private fun resume(id: Long) {
        pauseFlags[id]?.set(false)
        scope.launch {
            val dao = AppDatabase.get(this@DownloadService).downloadDao()
            val entity = dao.byId(id) ?: return@launch
            dao.setState(id, DownloadEntity.STATE_RUNNING)
            startEngine(entity, SettingsBridge.settings(this@DownloadService).first())
        }
    }

    private fun cancel(id: Long) {
        pauseFlags[id]?.set(true)
        jobs.remove(id)?.cancel()
        scope.launch {
            val dao = AppDatabase.get(this@DownloadService).downloadDao()
            val entity = dao.byId(id) ?: return@launch
            File(entity.dir, entity.filename + ".part").delete()
            File(entity.dir, entity.filename + ".part.vdmctl").delete()
            dao.delete(entity)
        }
    }

    private suspend fun daoSetState(id: Long, state: String) {
        AppDatabase.get(this).downloadDao().setState(id, state)
    }

    private suspend fun daoObserve(): List<DownloadEntity> =
        AppDatabase.get(this).downloadDao().unfinished()

    override fun onDestroy() {
        emitter?.cancel()
        scope.cancel()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        const val ACTION_PAUSE = "com.velocitydm.app.PAUSE"
        const val ACTION_RESUME = "com.velocitydm.app.RESUME"
        const val ACTION_CANCEL = "com.velocitydm.app.CANCEL"
        const val ACTION_ENQUEUE = "com.velocitydm.app.ENQUEUE"
        const val EXTRA_ID = "id"
        const val EXTRA_URL = "url"
        const val NOTIFICATION_ID = 41
        const val MAX_CONCURRENT = 3

        /** Throttled engine snapshots shared with the UI (10..30 Hz cap). */
        internal val progressFlow = kotlinx.coroutines.flow.MutableSharedFlow<ProgressThrottler.Snapshot>(
            extraBufferCapacity = 64,
            onBufferOverflow = kotlinx.coroutines.channels.BufferOverflow.DROP_OLDEST,
        )

        @Volatile
        private var engineProgressBridge: ProgressThrottler? = null
        internal val engineProgress: kotlinx.coroutines.flow.SharedFlow<ProgressThrottler.Snapshot>
            get() = engineProgressBridge?.flow
                ?: kotlinx.coroutines.flow.MutableSharedFlow(0, 64, kotlinx.coroutines.channels.BufferOverflow.DROP_OLDEST)

        fun intent(context: Context): Intent = Intent(context, DownloadService::class.java)

        fun actionIntent(context: Context, action: String, id: Long): Intent =
            Intent(context, DownloadService::class.java).setAction(action).putExtra(EXTRA_ID, id)

        fun sendAction(context: Context, action: String, id: Long) {
            context.startService(actionIntent(context, action, id))
        }

        /** Queues a URL by inserting a row and starting the service. */
        fun enqueue(context: Context, url: String) {
            val intent = Intent(context, DownloadService::class.java).setAction(ACTION_ENQUEUE).putExtra(EXTRA_URL, url)
            context.startService(intent)
        }

        fun enqueueId(context: Context, id: Long) {
            context.startService(actionIntent(context, ACTION_ENQUEUE, id).putExtra(EXTRA_ID, id))
        }

        private fun parseMirrors(json: String): List<String> = try {
            val arr = org.json.JSONArray(json)
            (0 until arr.length()).map { arr.getString(it) }
        } catch (e: Exception) {
            emptyList()
        }

        private fun parseChecksum(raw: String): Pair<String, String>? {
            val idx = raw.indexOf(':')
            if (idx <= 0) return null
            return raw.substring(0, idx).trim() to raw.substring(idx + 1).trim()
        }
    }
}

/** Blocking-read snapshot used when constructing engines. */
data class AppSettingsSnapshot(
    val downloadDir: String,
    val defaultParts: Int,
    val speedLimitBps: Long,
    val refreshHz: Int,
)

object SettingsBridge {
    fun settings(context: Context): kotlinx.coroutines.flow.Flow<AppSettingsSnapshot> =
        com.velocitydm.app.data.SettingsRepository(context).settings.let { flow ->
            kotlinx.coroutines.flow.flow {
                flow.collect { s ->
                    emit(AppSettingsSnapshot(s.downloadDir, s.defaultParts, s.speedLimitBps, s.refreshHz))
                }
            }
        }
}
