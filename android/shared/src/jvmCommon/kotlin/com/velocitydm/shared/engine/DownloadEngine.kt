package com.velocitydm.shared.engine

/**
 * The engine contract both download modes implement (S8-W1):
 * [SegmentEngine] (local Kotlin engine, files land on this device)
 * and `RemoteEngine` (a remote-control client of a desktop daemon over
 * the authenticated LAN RPC). The ViewModel depends on this surface
 * only, so the mode switch is a construction-time decision.
 */
interface DownloadEngine {
    /** Live task list (updated in place; snapshot with toList()). */
    val tasks: MutableList<TaskState>

    /** Invoked on every task update (drives UI + notifications). */
    var listener: ((TaskState) -> Unit)?

    /** Add a download; returns the task id. Remote engines ignore
     *  [dirPath] (the daemon's downloadDir owns placement). */
    suspend fun add(url: String, dirPath: String, opts: DownloadOptions): Long

    fun pause(id: Long)

    suspend fun resume(id: Long)

    suspend fun remove(id: Long)
}
