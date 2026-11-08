package com.velocitydm.app.engine

// SPDX-License-Identifier: GPL-3.0-or-later

import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import java.io.File
import java.io.RandomAccessFile
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

data class EngineSpec(
    val url: String,
    val mirrors: List<String> = emptyList(),
    val filename: String,
    val dir: File,
    val parts: Int,
    val rateLimitBps: Long,
    val checksum: Pair<String, String>? = null, // algorithm to expected hex
    val wifiOnly: Boolean = false,
)

sealed class EngineResult {
    data class Success(val file: File, val bytes: Long) : EngineResult()
    data class Failure(val error: String) : EngineResult()
}

/**
 * Segmented download engine: shared part queue (work-stealing: whichever
 * worker finishes first claims the next unowned part), scatter writes at
 * absolute offsets, `.part.vdmctl` resume state, bounded retries, and a
 * cooperative pause flag. Progress reaches the UI only through the
 * [ProgressThrottler] so the refresh rate stays capped.
 */
class DownloadEngine(
    private val spec: EngineSpec,
    private val client: HttpRangeClient,
    private val limiter: RateLimiter,
    private val throttler: ProgressThrottler,
    private val pauseFlag: java.util.concurrent.atomic.AtomicBoolean,
    private val onCtlTick: (String) -> Unit = {},
) {
    private val urls = listOf(spec.url) + spec.mirrors
    private val activeWorkers = AtomicLong(0)

    suspend fun run(): EngineResult = try {
        withContext(Dispatchers.IO) { runInternal() }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        EngineResult.Failure(e.message ?: e.javaClass.simpleName)
    }

    private suspend fun runInternal(): EngineResult {
        val probe = client.probeSuspend(spec.url)
        if (probe.total <= 0 && !probe.acceptRanges) {
            return runSingleStream(probe)
        }
        return runSegmented(probe)
    }

    /** Small or non-range files: one open-ended stream until EOF. */
    private suspend fun runSingleStream(probe: Probe): EngineResult {
        val finalFile = File(spec.dir, spec.filename)
        val partFile = File(spec.dir, spec.filename + ".part")
        spec.dir.mkdirs()
        var attempt = 0
        var fetched = 0L
        while (attempt < 5) {
            attempt += 1
            if (pauseFlag.get()) return EngineResult.Failure("paused")
            try {
                client.open(urls[(attempt - 1) % urls.size], fetched, null).use { resp ->
                    if (!resp.isSuccessful && resp.code != 206) {
                        throw java.io.IOException("HTTP ${resp.code}")
                    }
                    val source = resp.body?.source() ?: throw java.io.IOException("empty body")
                    RandomAccessFile(partFile, "rw").use { raf ->
                        val buf = ByteArray(64 * 1024)
                        var pos = fetched
                        while (true) {
                            if (pauseFlag.get()) {
                                saveSingleCtl(probe, fetched)
                                return EngineResult.Failure("paused")
                            }
                            val granted = limiter.acquire(buf.size)
                            val n = source.read(buf, 0, granted)
                            if (n <= 0) break
                            raf.seek(pos)
                            raf.write(buf, 0, n)
                            pos += n
                            fetched = pos
                            throttler.update(pos, probe.total)
                        }
                    }
                }
            } catch (e: Exception) {
                if (e is CancellationException) throw e
                Log.w(TAG, "single-stream attempt $attempt failed: ${e.message}")
                delay(250L shl attempt.coerceAtMost(4))
            }
        }
        if (fetched <= 0) return EngineResult.Failure("no data received")
        if (!finalize(partFile, finalFile)) return EngineResult.Failure("checksum mismatch")
        throttler.finished("completed")
        return EngineResult.Success(finalFile, fetched)
    }

    private fun saveSingleCtl(probe: Probe, done: Long) {
        val ctlFile = File(spec.dir, spec.filename + ".part.vdmctl")
        VdmCtl(
            url = spec.url,
            etag = probe.etag,
            total = probe.total,
            parts = mutableListOf(VdmPart(0, if (probe.total > 0) probe.total - 1 else Long.MAX_VALUE, done)),
        ).save(ctlFile)
    }

    private suspend fun runSegmented(probe: Probe): EngineResult {
        spec.dir.mkdirs()
        val partFile = File(spec.dir, spec.filename + ".part")
        val ctlFile = File(spec.dir, spec.filename + ".part.vdmctl")
        val total = probe.total
        val parts = resumeOrSplit(probe, partFile, ctlFile, total)
        val doneBytes = parts.sumOf { it.done }
        throttler.update(doneBytes, total)
        throttler.track(0)

        val queue = PartQueue(parts)
        RandomAccessFile(partFile, "rw").use { raf ->
            coroutineScope {
                val workers = (0 until spec.parts.coerceIn(1, 16)).map {
                    async { SegmentWorker(queue, spec, client, limiter, raf, pauseFlag, throttler).run() }
                }
                val ctlTicker = async {
                    while (queue.pending() && !queue.allDone()) {
                        delay(2000)
                        if (pauseFlag.get()) break
                        VdmCtl(spec.url, probe.etag, total, parts).save(ctlFile)
                    }
                }
                workers.awaitAll()
                ctlTicker.cancel()
            }
        }
        VdmCtl(spec.url, probe.etag, total, parts).save(ctlFile)

        if (pauseFlag.get()) return EngineResult.Failure("paused")
        val missing = parts.sumOf { it.partLength() - it.done }
        if (missing > 0) {
            return EngineResult.Failure("incomplete after retries: $missing bytes missing")
        }
        if (!finalize(partFile, File(spec.dir, spec.filename))) {
            return EngineResult.Failure("checksum mismatch")
        }
        VdmCtl.remove(ctlFile)
        throttler.finished("completed")
        return EngineResult.Success(File(spec.dir, spec.filename), total)
    }

    private fun resumeOrSplit(
        probe: Probe,
        partFile: File,
        ctlFile: File,
        total: Long,
    ): MutableList<VdmPart> {
        val existing = VdmCtl.load(ctlFile)
        if (existing != null && existing.url == spec.url && existing.total == total &&
            (existing.etag == null || probe.etag == null || existing.etag == probe.etag)
        ) {
            if (!partFile.exists()) partFile.createNewFile()
            return existing.parts
        }
        val count = spec.parts.coerceIn(1, 16)
        val partSize = (total / count).coerceAtLeast(1)
        val list = mutableListOf<VdmPart>()
        var start = 0L
        while (start < total) {
            val end = minOf(start + partSize, total) - 1
            list.add(VdmPart(start, end, 0))
            start = end + 1
        }
        if (!partFile.exists()) {
            partFile.outputStream().use { out ->
                // Sparse allocation: seek to the end instead of writing zeros.
                java.io.RandomAccessFile(partFile, "rw").setLength(total)
            }
        }
        return list
    }

    private fun finalize(partFile: File, finalFile: File): Boolean {
        if (finalFile.exists()) finalFile.delete()
        if (spec.checksum != null) {
            val (algorithm, expected) = spec.checksum
            val actual = when (algorithm.lowercase()) {
                "sha256", "sha-256" -> Checksum.sha256Hex(partFile)
                "md5" -> Checksum.md5Hex(partFile)
                else -> return true
            }
            if (!actual.equals(expected, ignoreCase = true)) {
                Log.w(TAG, "checksum mismatch: $actual != $expected")
                return false
            }
        }
        return partFile.renameTo(finalFile)
    }

    companion object {
        private const val TAG = "DownloadEngine"
    }
}

/** Shared work-stealing part queue: idle workers claim unowned parts. */
class PartQueue(private val parts: MutableList<VdmPart>) {
    private val owners = ConcurrentHashMap<Int, Boolean>()

    fun claim(): Pair<Int, VdmPart>? {
        for (i in parts.indices) {
            val part = parts[i]
            if (owners.putIfAbsent(i, true) == null) {
                if (part.done < part.partLength()) {
                    return i to part
                }
                owners.remove(i)
            }
        }
        return null
    }

    fun release(index: Int) = owners.remove(index)
    fun pending(): Boolean = parts.any { it.done < it.partLength() }
    fun allDone(): Boolean = parts.all { it.done >= it.partLength() }

    /** Live sum of bytes on disk across all parts (workers call per chunk). */
    fun doneSum(): Long = parts.sumOf { it.done }

    fun totalSum(): Long = parts.sumOf { it.partLength() }
}

fun VdmPart.partLength(): Long = end - start + 1
