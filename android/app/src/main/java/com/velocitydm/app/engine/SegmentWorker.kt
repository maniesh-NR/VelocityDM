package com.velocitydm.app.engine

// SPDX-License-Identifier: GPL-3.0-or-later

import android.util.Log
import kotlinx.coroutines.delay
import java.io.RandomAccessFile
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Fetches claimed parts end to end with bounded retries. Every chunk is
 * written at its absolute file offset (no full-file buffering) and the part
 * progress advances with the disk content, so a paused or failed worker
 * leaves a resume point exactly where the bytes end.
 */
class SegmentWorker(
    private val queue: PartQueue,
    private val spec: EngineSpec,
    private val client: HttpRangeClient,
    private val limiter: RateLimiter,
    private val raf: RandomAccessFile,
    private val pauseFlag: AtomicBoolean,
    private val throttler: ProgressThrottler,
) {
    private val urls = listOf(spec.url) + spec.mirrors

    suspend fun run() {
        while (!pauseFlag.get()) {
            val claim = queue.claim() ?: return
            val (index, part) = claim
            try {
                fetchPart(index, part)
                queue.release(index)
            } catch (e: Exception) {
                queue.release(index)
                if (e is kotlinx.coroutines.CancellationException) throw e
                Log.w(TAG, "worker part $index failed: ${e.message}")
                if (!pauseFlag.get()) delay(250)
            }
        }
    }

    private suspend fun fetchPart(index: Int, part: VdmPart) {
        var attempt = 0
        var fetched = part.done
        while (attempt < 5 && fetched < part.partLength()) {
            attempt += 1
            if (pauseFlag.get()) return
            val resumeFrom = part.start + fetched
            val rangeEnd = if (part.end == Long.MAX_VALUE) null else part.end
            try {
                client.open(urls[(attempt - 1) % urls.size], resumeFrom, rangeEnd).use { resp ->
                    if (!resp.isSuccessful && resp.code != 206) {
                        throw java.io.IOException("HTTP ${resp.code}")
                    }
                    val source = resp.body?.source() ?: throw java.io.IOException("empty body")
                    val buf = ByteArray(64 * 1024)
                    var pos = resumeFrom
                    while (true) {
                        if (pauseFlag.get()) return
                        val granted = limiter.acquire(buf.size)
                        val n = source.read(buf, 0, granted)
                        if (n <= 0) break
                        synchronized(raf) {
                            raf.seek(pos)
                            raf.write(buf, 0, n)
                        }
                        pos += n
                        val written = pos - part.start
                        part.done = written
                        throttler.update(queue.doneSum(), queue.totalSum())
                    }
                    fetched = pos - part.start
                }
            } catch (e: Exception) {
                if (e is kotlinx.coroutines.CancellationException) throw e
                Log.w(TAG, "part $index attempt $attempt failed at +$fetched: ${e.message}")
                delay(200L shl attempt.coerceAtMost(4))
            }
        }
    }

    companion object {
        private const val TAG = "SegmentWorker"
    }
}
