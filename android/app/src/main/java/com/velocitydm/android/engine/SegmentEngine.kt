package com.velocitydm.android.engine

import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.io.RandomAccessFile
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Segmented HTTP download engine (Android side of VelocityDM).
 *
 * Mirrors the Rust engine's model: HEAD/ranged probe, N worker coroutines
 * leasing chunks from a work-stealing range pool, mirror failover, per-task
 * token-bucket rate limit, crash-safe `.part`/`.vdmctl` resume state.
 */
data class DownloadOptions(
    val connections: Int = 4,
    val mirrors: List<String> = emptyList(),
    val headers: Map<String, String> = emptyMap(),
    val rateLimitBps: Long = 0,
    val filename: String? = null,
)

data class TaskState(
    val id: Long,
    val url: String,
    val filename: String,
    val dir: File,
    val size: Long,
    var done: Long = 0,
    var speed: Long = 0,
    var state: String = "queued", // queued|probing|downloading|paused|completed|failed
    var error: String? = null,
)

class SegmentEngine(private val scope: CoroutineScope) {
    private val http = OkHttpClient.Builder()
        .connectTimeout(20, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    val tasks = mutableListOf<TaskState>()
    private val mutex = Mutex()
    private val jobs = mutableMapOf<Long, Job>()
    private val stops = mutableMapOf<Long, AtomicBoolean>()
    var listener: ((TaskState) -> Unit)? = null

    suspend fun add(url: String, dir: File, opts: DownloadOptions): Long = mutex.withLock {
        val id = (tasks.maxOfOrNull { it.id } ?: 0L) + 1
        val filename = opts.filename ?: url.substringAfterLast('/').substringBefore('?')
            .ifBlank { "download" }
        tasks.add(TaskState(id, url, filename, dir, 0))
        jobs[id] = scope.launch { run(id, opts) }
        id
    }

    fun pause(id: Long) { stops[id]?.set(true) }

    suspend fun resume(id: Long) = mutex.withLock {
        val t = tasks.firstOrNull { it.id == id } ?: return
        if (t.state == "paused" || t.state == "failed") {
            stops[id] = AtomicBoolean(false)
            jobs[id] = scope.launch { run(id, DownloadOptions()) }
        }
    }

    suspend fun remove(id: Long) = mutex.withLock {
        stops[id]?.set(true)
        jobs.remove(id)?.cancel()
        tasks.removeAll { it.id == id }
    }

    // ----------------------------------------------------------------- core

    private suspend fun run(id: Long, opts: DownloadOptions) {
        val t = tasks.firstOrNull { it.id == id } ?: return
        val stop = AtomicBoolean(false)
        stops[id] = stop
        t.state = "probing"
        emit(t)
        try {
            val head = probe(t.url, opts)
            t.size = head.size
            val file = File(t.dir, t.filename)
            val part = File(t.dir, "${t.filename}.part")
            val ctl = File(t.dir, "${t.filename}.vdmctl")
            if (head.size <= 0L || !head.acceptsRanges) {
                singleStream(t, opts, stop, part, file)
            } else {
                segmented(t, opts, stop, head, part, file, ctl)
            }
            if (stop.get()) {
                t.state = "paused"
            } else if (part.exists()) {
                part.renameTo(file)
                ctl.delete()
                t.done = t.size
                t.state = "completed"
            }
        } catch (e: Exception) {
            t.state = if (stop.get()) "paused" else "failed"
            t.error = e.message
        }
        emit(t)
    }

    private data class Probe(val size: Long, val acceptsRanges: Boolean, val etag: String?)

    private fun probe(url: String, opts: DownloadOptions): Probe {
        val b = Request.Builder().url(url).head()
        opts.headers.forEach { (k, v) -> b.header(k, v) }
        http.newCall(b.build()).execute().use { r ->
            val size = r.header("Content-Length")?.toLongOrNull() ?: 0L
            val ranges = r.header("Accept-Ranges")?.contains("bytes") == true
            var etag = r.header("ETag")
            if (size > 0 && ranges) return Probe(size, ranges, etag)
        }
        // fallback ranged GET
        val b = Request.Builder().url(url).get().header("Range", "bytes=0-0")
        opts.headers.forEach { (k, v) -> b.header(k, v) }
        http.newCall(b.build()).execute().use { r ->
            if (r.code == 206) {
                val total = r.header("Content-Range")?.substringAfterLast('/')?.toLongOrNull() ?: 0L
                return Probe(total, true, r.header("ETag"))
            }
            val size = r.header("Content-Length")?.toLongOrNull() ?: 0L
            return Probe(size, false, null)
        }
    }

    private fun singleStream(t: TaskState, opts: DownloadOptions, stop: AtomicBoolean, part: File, out: File) {
        val b = Request.Builder().url(t.url).get()
        opts.headers.forEach { (k, v) -> b.header(k, v) }
        http.newCall(b.build()).execute().use { r ->
            if (!r.isSuccessful) throw IOException("HTTP ${r.code}")
            val body = r.body ?: throw IOException("empty body")
            RandomAccessFile(part, "rw").use { f ->
                val buf = ByteArray(64 * 1024)
                var last = System.currentTimeMillis()
                var lastDone = 0L
                var done = 0L
                val input = body.byteStream()
                while (true) {
                    if (stop.get()) return
                    val n = input.read(buf)
                    if (n <= 0) break
                    f.write(buf, 0, n)
                    done += n
                    t.done = done
                    val now = System.currentTimeMillis()
                    if (now - last > 250) {
                        t.speed = (done - lastDone) * 1000 / (now - last)
                        last = now; lastDone = done
                        emit(t)
                    }
                }
            }
        }
    }

    private suspend fun segmented(
        t: TaskState,
        opts: DownloadOptions,
        stop: AtomicBoolean,
        head: Probe,
        part: File,
        out: File,
        ctl: File,
    ) {
        // resume state
        var intervals = listOf(0L to head.size - 1)
        if (ctl.exists() && part.exists()) {
            try {
                val st = JSONObject(ctl.readText())
                if (st.optString("etag", "") == (head.etag ?: "") || head.etag == null) {
                    val arr = st.getJSONArray("remaining")
                    intervals = (0 until arr.length()).map { i ->
                        val iv = arr.getJSONObject(i)
                        iv.getLong("start") to iv.getLong("end")
                    }
                }
            } catch (_: Exception) { }
        }
        if (part.exists() && intervals.size == 1 && intervals[0].first == 0L) {
            part.delete()
        }
        RandomAccessFile(part, "rw").use { raf -> raf.setLength(head.size) }

        val done = AtomicLong(head.size - intervals.sumOf { it.second - it.first + 1 })
        t.done = done.get()
        val mirrors = listOf(t.url) + opts.mirrors
        val lock = Any()
        val mirrorsIdx = java.util.concurrent.atomic.AtomicInteger(0)
        val bucket = TokenBucket(opts.rateLimitBps)

        fun saveState(remaining: List<Pair<Long, Long>>) {
            val st = JSONObject()
            st.put("url", t.url)
            st.put("size", head.size)
            st.put("etag", head.etag ?: "")
            val arr = JSONArray()
            remaining.forEach { (s, e) ->
                arr.put(JSONObject().put("start", s).put("end", e))
            }
            st.put("remaining", arr)
            ctl.writeText(st.toString())
        }

        val chans = Channel<Unit>(opts.connections)
        val workers = (0 until opts.connections.coerceIn(1, 8)).map { w ->
            scope.launch {
                var retries = 0
                while (!stop.get()) {
                    val lease: Pair<Long, Long>? = synchronized(lock) {
                        val idx = intervals.indexOfFirst { it.second - it.first + 1 > 0 }
                        if (idx < 0) null else {
                            val (s, e) = intervals[idx]
                            val leaseLen = minOf(4L * 1024 * 1024, e - s + 1)
                            val ls = e - leaseLen + 1
                            intervals[idx] = s to ls - 1
                            ls to e
                        }
                    }
                    if (lease == null) break
                    var (pos, end) = lease
                    val url = mirrors[Math.floorMod(mirrorsIdx.getAndIncrement(), mirrors.size)]
                    val b = Request.Builder().url(url).get()
                        .header("Range", "bytes=$pos-$end")
                    opts.headers.forEach { (k, v) -> b.header(k, v) }
                    try {
                        http.newCall(b.build()).execute().use { r ->
                            if (r.code != 206 && !(r.code == 200 && pos == 0L)) throw IOException("HTTP ${r.code}")
                            val body = r.body ?: throw IOException("empty body")
                            val input = body.byteStream()
                            val buf = ByteArray(64 * 1024)
                            synchronized(this@SegmentEngine) {
                                RandomAccessFile(part, "rw").use { f ->
                                    while (pos <= end) {
                                        if (stop.get()) {
                                            synchronized(lock) { intervals.add(pos to end) }
                                            return
                                        }
                                        val n = input.read(buf)
                                        if (n <= 0) throw IOException("short body")
                                        bucket.take(n.toLong())
                                        f.seek(pos)
                                        f.write(buf, 0, n)
                                        pos += n
                                        val d = done.addAndGet(n.toLong())
                                        t.done = d
                                    }
                                }
                            }
                        }
                        retries = 0
                    } catch (e: Exception) {
                        retries++
                        if (retries > 12) throw e
                        synchronized(lock) { intervals.add(pos to end) }
                        delay(minOf(250L shl (retries.coerceAtMost(4)), 4000L))
                        continue
                    }
                }
            }
        }
        // periodic progress + state saving while workers run
        val monitor = scope.launch {
            var last = System.currentTimeMillis()
            var lastDone = done.get()
            while (workers.any { it.isActive }) {
                delay(250)
                val now = System.currentTimeMillis()
                val d = done.get()
                t.speed = (d - lastDone) * 1000 / (now - last).coerceAtLeast(1)
                last = now; lastDone = d
                emit(t)
                synchronized(lock) { saveState(intervals.toList()) }
            }
        }
        workers.joinAll()
        monitor.cancel()
        if (!stop.get()) {
            synchronized(lock) {
                if (intervals.isNotEmpty() && intervals.any { it.second >= it.first }) {
                    throw IOException("download ended with missing ranges")
                }
            }
        } else {
            synchronized(lock) { saveState(intervals.toList()) }
        }
    }

    private fun emit(t: TaskState) {
        listener?.invoke(t.copy())
    }

    /** Simple token bucket for per-download limits. */
    class TokenBucket(private val rateBps: Long) {
        private var tokens = 0.0
        private var last = System.currentTimeMillis()

        fun take(n: Long) {
            if (rateBps <= 0) return
            while (true) {
                val now = System.currentTimeMillis()
                tokens = minOf(
                    tokens + (now - last) * rateBps / 1000.0,
                    256.0 * 1024
                )
                last = now
                if (tokens >= n) { tokens -= n; return }
                Thread.sleep(((n - tokens) * 1000 / rateBps).toLong().coerceIn(1, 100))
            }
        }
    }
}
