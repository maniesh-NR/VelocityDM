package com.velocitydm.shared.engine

/**
 * Shared task/options models (commonMain). Pure value types on String
 * paths — no `java.io.File` here, so every KMP target can hold task
 * state. The JVM engine resolves `File(dirPath)` where it needs the
 * filesystem.
 */

data class DownloadOptions(
    val connections: Int = 4,
    val mirrors: List<String> = emptyList(),
    val headers: Map<String, String> = emptyMap(),
    val rateLimitBps: Long = 0,
    val filename: String? = null,
)

/**
 * One download task's live state. Mutable fields (size/done/speed/
 * state/error) are updated by the engine's workers and snapshotted on
 * emit, mirroring the historic engine contract.
 *
 * S8-W1 additive fields (remote mode only; Kotlin defaults keep the
 * local engine and the strict .vdmctl schema untouched):
 *  - [notice]: the daemon's advisory hint (e.g. proxy warnings) — the
 *    desktop snapshot carries it; the local engine never sets it.
 *  - [userPaused]: whether a paused task was paused BY THE USER (the
 *    daemon's stop-window pauses with userPaused=false — resuming it
 *    would instantly re-stop). null for local tasks (treated as
 *    user-paused by the UI).
 */
data class TaskState(
    val id: Long,
    val url: String,
    val filename: String,
    val dirPath: String,
    var size: Long = 0,
    var done: Long = 0,
    var speed: Long = 0,
    var state: String = "queued", // queued|probing|downloading|paused|completed|failed|verifying|seeding
    var error: String? = null,
    var notice: String? = null,
    var userPaused: Boolean? = null,
)
