package com.velocitydm.shared.engine

import com.velocitydm.shared.net.filenameFromUrl
import com.velocitydm.shared.net.sanitizeFilename
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.io.RandomAccessFile
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/**
 * Segmented HTTP download engine (the JVM side of VelocityDM's Android
 * client).
 *
 * Mirrors the Rust engine's model: HEAD/ranged probe, N worker
 * coroutines leasing chunks from a work-stealing range pool, mirror
 * failover, per-task token-bucket rate limit, crash-safe
 * `.part`/`.vdmctl` resume state. Lives in the shared KMP module's
 * `jvmCommon` source set so both JVM-family targets (android + jvm)
 * compile it; the pure math (range pool, control-file schema, bucket)
 * is in commonMain and unit-tested there.
 *
 * S12-e hardening (gate 0005): the `.vdmctl` resume state is strictly
 * validated (bounds, coverage, unknown keys — violations restart from
 * zero; a missing server ETag only resumes on an exact size match),
 * and every filename passes [sanitizeFilename].
 */
class SegmentEngine(private val scope: CoroutineScope) : DownloadEngine {
    private val http = OkHttpClient.Builder()
        .connectTimeout(20, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    override val tasks = mutableListOf<TaskState>()
    private val mutex = Mutex()
    private val jobs = mutableMapOf<Long, Job>()
    private val stops = mutableMapOf<Long, AtomicBoolean>()
    override var listener: ((TaskState) -> Unit)? = null

    override suspend fun add(url: String, dirPath: String, opts: DownloadOptions): Long = mutex.withLock {
        val id = (tasks.maxOfOrNull { it.id } ?: 0L) + 1
        val filename = opts.filename?.let { sanitizeFilename(it) } ?: filenameFromUrl(url)
        tasks.add(TaskState(id, url, filename, dirPath))
        jobs[id] = scope.launch { run(id, opts) }
        id
    }

    override fun pause(id: Long) {
        stops[id]?.set(true)
    }

    override suspend fun resume(id: Long) = mutex.withLock {
        val t = tasks.firstOrNull { it.id == id } ?: return
        if (t.state == "paused" || t.state == "failed") {
            stops[id] = AtomicBoolean(false)
            jobs[id] = scope.launch { run(id, DownloadOptions()) }
        }
    }

    override suspend fun remove(id: Long) {
        mutex.withLock {
            stops[id]?.set(true)
            jobs.remove(id)?.cancel()
            tasks.removeAll { it.id == id }
        }
    }

    /**
     * S8-W1: pause every active task (graceful — workers check their
     * stop flag each 64 KiB and save resume state). Called when the
     * user switches the app from local to remote mode so no local
     * download keeps running invisibly; tasks resume when the user
     * switches back (crash-safe .part/.vdmctl state on disk).
     */
    fun stopAll() {
        stops.values.forEach { it.set(true) }
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
            val dir = File(t.dirPath)
            val file = File(dir, t.filename)
            val part = File(dir, "${t.filename}.part")
            val ctl = File(dir, "${t.filename}.vdmctl")
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
            val etag = r.header("ETag")
            if (size > 0 && ranges) return Probe(size, ranges, etag)
        }
        // fallback ranged GET
        val b2 = Request.Builder().url(url).get().header("Range", "bytes=0-0")
        opts.headers.forEach { (k, v) -> b2.header(k, v) }
        http.newCall(b2.build()).execute().use { r ->
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
        // resume state — strictly validated (gate 0005): a tampered or
        // stale .vdmctl restarts the download instead of seeking wild
        // offsets or resuming a changed resource.
        var intervals = mutableListOf(0L to head.size - 1)
        if (ctl.exists() && part.exists()) {
            val stored = Vdmctl.parse(ctl.readText())
            if (Vdmctl.resumableWith(stored, head.size, head.etag) && stored != null) {
                intervals = stored.remaining.map { it.start to it.end }.toMutableList()
            }
        }
        if (part.exists() && intervals.size == 1 && intervals[0].first == 0L) {
            part.delete()
        }
        RandomAccessFile(part, "rw").use { raf -> raf.setLength(head.size) }

        val done = AtomicLong(head.size - poolBytes(intervals))
        t.done = done.get()
        val mirrors = listOf(t.url) + opts.mirrors
        val lock = Any()
        val mirrorsIdx = AtomicInteger(0)
        val bucket = TokenBucket(opts.rateLimitBps)

        fun saveState(remaining: List<Interval>) {
            val f = VdmctlFile(
                url = t.url,
                size = head.size,
                etag = head.etag ?: "",
                remaining = remaining.map { VdmctlInterval(it.first, it.second) },
            )
            ctl.writeText(Vdmctl.encode(f))
        }

        val workers = (0 until opts.connections.coerceIn(1, 8)).map {
            scope.launch {
                var retries = 0
                while (!stop.get()) {
                    val lease: Interval? = synchronized(lock) { leaseInterval(intervals, 4L * 1024 * 1024) }
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
                                            synchronized(lock) { releaseInterval(intervals, pos to end) }
                                            return@launch
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
                        synchronized(lock) { releaseInterval(intervals, pos to end) }
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
                if (!poolExhausted(intervals)) {
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

    /**
     * Token-bucket rate limiter; the wait math lives in commonMain
     * (`tokenBucketStep`) so it is unit-tested with a virtual clock.
     */
    class TokenBucket(private val rateBps: Long) {
        private var tokens = 0.0
        private var last = System.currentTimeMillis()

        fun take(n: Long) {
            if (rateBps <= 0) return
            while (true) {
                val step = tokenBucketStep(
                    tokens = tokens,
                    lastRefillMs = last,
                    nowMs = System.currentTimeMillis(),
                    rateBps = rateBps,
                    demand = n,
                )
                tokens = step.tokens
                last = step.lastRefillMs
                if (step.waitMs <= 0L) return
                try {
                    Thread.sleep(step.waitMs)
                } catch (_: InterruptedException) {
                    Thread.currentThread().interrupt()
                    return
                }
            }
        }
    }
}
