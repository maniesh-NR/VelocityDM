package com.velocitydm.app.engine

// SPDX-License-Identifier: GPL-3.0-or-later

import android.util.Log
import kotlinx.coroutines.delay
import java.io.File
import java.util.concurrent.atomic.AtomicLong

/** Token-bucket byte rate limiter; rate 0 means unlimited. */
class RateLimiter(bytesPerSec: Long) {
    private val rate = AtomicLong(bytesPerSec.coerceAtLeast(0))
    private val tokens = AtomicLong(0)
    private val last = AtomicLong(System.nanoTime())

    fun setRate(bytesPerSec: Long) = rate.set(bytesPerSec.coerceAtLeast(0))

    suspend fun acquire(want: Int): Int {
        val r = rate.get()
        if (r <= 0) return want
        while (true) {
            val now = System.nanoTime()
            val lastN = last.get()
            val elapsedSecs = (now - lastN) / 1_000_000_000.0
            if (elapsedSecs > 0 && last.compareAndSet(lastN, now)) {
                val capacity = r * 2 + MIN_BURST
                val current = tokens.get()
                val refilled = (current + (elapsedSecs * r).toLong()).coerceAtMost(capacity)
                tokens.set(refilled)
            }
            val available = tokens.get()
            if (available >= 1) {
                val grant = minOf(want.toLong(), available)
                if (tokens.compareAndSet(available, available - grant)) {
                    return grant.toInt().coerceAtLeast(1)
                }
                continue
            }
            delay(20)
        }
    }

    companion object {
        private const val MIN_BURST = 64L * 1024
    }
}

/**
 * Emits engine progress snapshots to the UI at a fixed refresh rate clamped
 * to 10..30 Hz. This is the engine/UI decoupling point: workers push raw
 * counters, the UI receives at most `refreshHz` updates per second.
 */
class ProgressThrottler(
    private val scope: kotlinx.coroutines.CoroutineScope,
    initialHz: Int,
) {
    private val hz = AtomicLong(initialHz.coerceIn(10, 30))

    data class Snapshot(
        val taskId: Long,
        val downloaded: Long,
        val total: Long,
        val speedBps: Long,
        val state: String,
    )

    val flow = kotlinx.coroutines.flow.MutableSharedFlow<Snapshot>(
        replay = 0,
        extraBufferCapacity = 1,
        onBufferOverflow = kotlinx.coroutines.channels.BufferOverflow.DROP_OLDEST,
    )

    fun setRefreshHz(value: Int) = hz.set(value.coerceIn(10, 30))

    private val downloaded = AtomicLong(0)
    private val totalBytes = AtomicLong(0)
    private val speed = AtomicLong(0)
    private var taskId = 0L
    private var state = "running"
    private var sampler: kotlinx.coroutines.Job? = null

    fun track(id: Long) {
        taskId = id
        downloaded.set(0)
        totalBytes.set(0)
        speed.set(0)
        state = "running"
        if (sampler == null) {
            sampler = scope.launch {
                var lastBytes = 0L
                while (true) {
                    delay(1000)
                    val now = downloaded.get()
                    speed.set(now - lastBytes)
                    lastBytes = now
                }
            }
        }
    }

    fun update(done: Long, total: Long) {
        downloaded.set(done)
        totalBytes.set(total)
    }

    fun finished(newState: String) {
        state = newState
        speed.set(0)
    }

    suspend fun emitLoop(onStop: () -> Boolean) {
        val intervalMs = 1000L / hz.get().coerceAtLeast(1)
        while (!onStop()) {
            flow.emit(Snapshot(taskId, downloaded.get(), totalBytes.get(), speed.get(), state))
            delay(intervalMs)
        }
        flow.emit(Snapshot(taskId, downloaded.get(), totalBytes.get(), 0, state))
    }
}

/** Streaming SHA-256 / MD5 digests for post-download verification. */
object Checksum {
    fun sha256Hex(file: File): String = digest(file, "SHA-256")
    fun md5Hex(file: File): String = digest(file, "MD5")

    private fun digest(file: File, algorithm: String): String {
        val md = java.security.MessageDigest.getInstance(algorithm)
        file.inputStream().use { input ->
            val buf = ByteArray(1024 * 1024)
            while (true) {
                val n = input.read(buf)
                if (n <= 0) break
                md.update(buf, 0, n)
            }
        }
        return md.digest().joinToString("") { "%02x".format(it) }
    }
}

/** `<name>.part.vdmctl` resume state: part map persisted atomically. */
data class VdmPart(val start: Long, val end: Long, var done: Long)

data class VdmCtl(
    val url: String,
    val etag: String?,
    val total: Long,
    val parts: MutableList<VdmPart>,
) {
    fun save(target: File) {
        val arr = org.json.JSONArray()
        for (p in parts) {
            arr.put(
                org.json.JSONObject()
                    .put("start", p.start)
                    .put("end", p.end)
                    .put("done", p.done),
            )
        }
        val obj = org.json.JSONObject()
            .put("version", 1)
            .put("url", url)
            .put("etag", etag ?: org.json.JSONObject.NULL)
            .put("total", total)
            .put("parts", arr)
        val tmp = File(target.parentFile, target.name + ".tmp")
        tmp.writeText(obj.toString())
        if (!tmp.renameTo(target)) {
            target.delete()
            tmp.renameTo(target)
        }
    }

    companion object {
        private const val TAG = "VdmCtl"

        fun load(target: File): VdmCtl? = try {
            val obj = org.json.JSONObject(target.readText())
            if (obj.optInt("version") != 1) return null
            val parts = mutableListOf<VdmPart>()
            val arr = obj.optJSONArray("parts") ?: return null
            for (i in 0 until arr.length()) {
                val p = arr.getJSONObject(i)
                parts.add(VdmPart(p.getLong("start"), p.getLong("end"), p.getLong("done")))
            }
            VdmCtl(
                url = obj.getString("url"),
                etag = if (obj.isNull("etag")) null else obj.optString("etag"),
                total = obj.getLong("total"),
                parts = parts,
            )
        } catch (e: Exception) {
            Log.w(TAG, "unreadable ctl: ${e.message}")
            null
        }

        fun remove(target: File) {
            target.delete()
        }
    }
}
