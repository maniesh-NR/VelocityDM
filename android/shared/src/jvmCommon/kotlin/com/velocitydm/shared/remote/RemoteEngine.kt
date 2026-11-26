package com.velocitydm.shared.remote

import com.velocitydm.shared.engine.DownloadEngine
import com.velocitydm.shared.engine.DownloadOptions
import com.velocitydm.shared.engine.TaskState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/**
 * Remote-control engine (S8-W1): the phone drives a desktop daemon
 * over the authenticated LAN RPC instead of downloading locally.
 * Implements the gate-0010 SSE consumer contract:
 *
 *  - initial `downloads.list`; a dead daemon at launch marks the
 *    surface stale (idempotent — the probe loop keeps trying, rows
 *    stay frozen, the banner shows);
 *  - `tasks` ticks merge IN PLACE keyed by id (never a per-tick full
 *    reset); a tick revealing membership change (unknown/missing id)
 *    or metadata drift schedules a coalesced 150 ms re-list;
 *  - non-tick events (task-added/state/completed/failed/removed,
 *    config-changed, queue-drained) schedule the same coalesced
 *    re-list — the server's tick change-key (state|done|speed|error)
 *    cannot carry notice changes, so the re-list is the truth source;
 *  - the stream loop re-lists BEFORE reading events after every
 *    (re)connect (the resync window — ticks arriving mid-resync are
 *    dropped);
 *  - stream death or RPC failure freezes the rows and probes every
 *    2 s; probe failures never mutate the task list; recovery = probe
 *    success → full re-list → stream reopen.
 *
 * Adds send url + filename only (the daemon's downloadDir owns
 * placement). Remote and local task lists are never merged (the id
 * namespaces are independent).
 */
class RemoteEngine(
    private val scope: CoroutineScope,
    config: RemoteConfig,
    store: TofuStore,
) : DownloadEngine {

    override val tasks = mutableListOf<TaskState>()
    override var listener: ((TaskState) -> Unit)? = null

    /** Dead-daemon banner state (stale = frozen rows). */
    var stale: Boolean = false
        private set

    /** Distinct failure classes for the UI: unreachable / 401 / TLS. */
    var lastFailure: String? = null
        private set

    var statusListener: ((stale: Boolean, failure: String?) -> Unit)? = null

    private val rpc = RemoteRpc(config, TofuTrustManager(store))
    private val mutex = Mutex()
    private var streamJob: Job? = null
    private var relistJob: Job? = null
    @Volatile
    private var resyncing = true
    private val lastKey = HashMap<Long, String>()

    fun start() {
        openStream()
    }

    fun stop() {
        streamJob?.cancel()
        relistJob?.cancel()
    }

    override suspend fun add(url: String, dirPath: String, opts: DownloadOptions): Long {
        // dirPath is intentionally ignored: the daemon's downloadDir
        // owns placement for remote adds.
        val params = if (RemoteMapper.isMagnet(url)) {
            RemoteMapper.magnetParams(url)
        } else {
            RemoteMapper.addParams(url, opts)
        }
        val method = if (RemoteMapper.isMagnet(url)) "downloads.addTorrent" else "downloads.add"
        val result = rpc.call(method, params)
        return result.fold(
            onSuccess = { el ->
                val id = (el as? JsonObject)?.get("id")?.jsonPrimitive?.longOrNull ?: -1L
                scheduleRelist()
                id
            },
            onFailure = {
                lastFailure = it.message
                statusListener?.invoke(stale, it.message)
                -1L
            },
        )
    }

    override fun pause(id: Long) {
        scope.launch { rpc.call("downloads.pause", idParams(id)); scheduleRelist() }
    }

    override suspend fun resume(id: Long) {
        rpc.call("downloads.resume", idParams(id))
        scheduleRelist()
    }

    override suspend fun remove(id: Long) {
        rpc.call("downloads.remove", idParams(id))
        scheduleRelist()
    }

    private fun idParams(id: Long): JsonObject =
        JsonObject(mapOf("id" to kotlinx.serialization.json.JsonPrimitive(id)))

    // ------------------------------------------------------------ stream

    private fun openStream() {
        streamJob?.cancel()
        streamJob = scope.launch {
            while (isActive) {
                resyncing = true
                if (!relist(markStaleOnFail = true)) {
                    // stale: frozen rows + the 2 s probe; failures never
                    // mutate the task list
                    delay(PROBE_MS)
                    continue
                }
                try {
                    rpc.readEvents { payload -> onPayload(payload) }
                    markStale("stream closed by the daemon")
                } catch (e: Exception) {
                    markStale(failureText(e))
                }
                delay(PROBE_MS)
            }
        }
    }

    private fun onPayload(payload: String) {
        val (type, tickTasks) = RemoteMapper.parseEvent(payload) ?: return
        if (type == "tasks") {
            if (resyncing) return // dropped inside the resync window
            mergeTicks(tickTasks)
        } else {
            // membership/metadata truth rides the coalesced re-list
            scheduleRelist()
        }
    }

    private fun mergeTicks(incoming: List<TaskState>) {
        scope.launch {
            mutex.withLock {
                // Ticks carry only CHANGED snapshots: a row absent from
                // this tick is untouched, not removed — removals arrive
                // as task-removed events (coalesced re-list).
                for (t in incoming) {
                    val idx = tasks.indexOfFirst { it.id == t.id }
                    if (idx < 0) {
                        // unknown id: membership change → truth via re-list
                        scheduleRelist()
                        continue
                    }
                    val cur = tasks[idx]
                    if (key(cur) != key(t)) {
                        tasks[idx] = t
                        emitLocked(t)
                    }
                }
            }
        }
    }

    private fun scheduleRelist() {
        relistJob?.cancel()
        relistJob = scope.launch {
            delay(COALESCE_MS)
            relist(markStaleOnFail = false)
        }
    }

    private suspend fun relist(markStaleOnFail: Boolean): Boolean {
        val result = rpc.call("downloads.list", JsonObject(emptyMap()))
        return result.fold(
            onSuccess = { el ->
                val list = when (el) {
                    is kotlinx.serialization.json.JsonArray -> RemoteMapper.listFromElement(el)
                    else -> null
                } ?: return@fold false
                mutex.withLock {
                    val byId = HashMap<Long, TaskState>()
                    list.forEach { byId[it.id] = it }
                    // in-place update keyed by id, append new at the end
                    // (append-never-reorder)
                    for (i in tasks.indices) {
                        byId[tasks[i].id]?.let { fresh ->
                            if (key(tasks[i]) != key(fresh)) {
                                tasks[i] = fresh
                                emitLocked(fresh)
                            }
                        }
                    }
                    byId.values.filter { fresh -> tasks.none { it.id == fresh.id } }
                        .forEach { fresh ->
                            tasks.add(fresh)
                            emitLocked(fresh)
                        }
                    tasks.removeAll { cur -> byId[cur.id] == null }
                }
                if (stale) {
                    stale = false
                    lastFailure = null
                    statusListener?.invoke(false, null)
                }
                true
            },
            onFailure = {
                if (markStaleOnFail) markStale(failureText(it))
                false
            },
        )
    }

    private fun markStale(failure: String) {
        if (!stale) {
            stale = true
            lastFailure = failure
            statusListener?.invoke(true, failure)
        } else {
            lastFailure = failure
        }
    }

    private fun failureText(e: Throwable): String = when (e) {
        is RemoteRpc.RpcFailure.Unreachable -> "daemon unreachable — is the desktop app running?"
        is RemoteRpc.RpcFailure.Rejected -> e.message ?: "rejected — check the token / certificate"
        else -> e.message ?: "connection lost"
    }

    private fun key(t: TaskState): String =
        "${t.state}|${t.done}|${t.speed}|${t.error}|${t.notice}|${t.userPaused}"

    private fun emitLocked(t: TaskState) {
        listener?.invoke(t.copy())
    }

    companion object {
        private const val PROBE_MS = 2000L
        private const val COALESCE_MS = 150L
    }
}
