package com.velocitydm.shared.remote

import com.velocitydm.shared.engine.DownloadOptions
import com.velocitydm.shared.engine.TaskState
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/**
 * Snapshot → [TaskState] mapping (S8-W1 spec §8, the explicit table).
 *
 * Direct: id, url, name→filename, dir→dirPath, size, done, speed.
 * Passthrough strings: state (the daemon's full state set INCLUDING
 * verifying/seeding, which the local engine never produces), error,
 * notice, and userPaused (null for local tasks — treated as
 * user-paused by the UI).
 *
 * Dropped for W1 (the Android card renders none of them today):
 * etaSecs, connections, peers, seeds, priority, startAt, checksumOk,
 * createdAt, kind, outPath. BT p/s is a recorded remote-side
 * limitation.
 */
object RemoteMapper {

    fun snapshotToTask(o: JsonObject): TaskState? {
        val id = o.long("id") ?: return null
        val url = o.string("url") ?: ""
        val name = o.string("name") ?: "download"
        return TaskState(
            id = id,
            url = url,
            filename = name,
            dirPath = o.string("dir") ?: "",
            size = o.long("size") ?: 0L,
            done = o.long("done") ?: 0L,
            speed = o.long("speed") ?: 0L,
            state = o.string("state") ?: "queued",
            error = o.string("error"),
            notice = o.string("notice"),
            userPaused = o.boolean("userPaused"),
        )
    }

    fun parseList(body: String): List<TaskState>? {
        val element = runCatching { Json.parseToJsonElement(body) }.getOrNull() ?: return null
        return listFromElement(element)
    }

    fun listFromElement(element: JsonElement): List<TaskState>? {
        val arr = runCatching { element.jsonArray }.getOrNull() ?: return null
        return arr.mapNotNull { el ->
            runCatching { el.jsonObject }.getOrNull()?.let { snapshotToTask(it) }
        }
    }

    /** Parse a `data:` SSE payload (a tasks tick carries a `tasks`
     *  array; other events return an empty list and keep their type
     *  for routing). */
    fun parseEvent(payload: String): Pair<String, List<TaskState>>? {
        val obj = runCatching { Json.parseToJsonElement(payload).jsonObject }.getOrNull()
            ?: return null
        val type = runCatching { obj.getValue("type").jsonPrimitive.content }.getOrNull()
            ?: return null
        val tasks = (obj["tasks"] as? JsonArray)?.mapNotNull { el ->
            runCatching { el.jsonObject }.getOrNull()?.let { snapshotToTask(it) }
        } ?: emptyList()
        return type to tasks
    }

    fun isMagnet(url: String): Boolean = url.startsWith("magnet:", ignoreCase = true)

    /** The remote add payload: url + filename only (the daemon's
     *  downloadDir owns placement — the phone's dirPath is never
     *  sent), rateLimitBps when set. */
    fun addParams(url: String, opts: DownloadOptions): JsonObject {
        val root = mutableMapOf<String, JsonElement>(
            "url" to JsonPrimitive(url),
        )
        val options = mutableMapOf<String, JsonElement>()
        if (opts.filename != null) options["filename"] = JsonPrimitive(opts.filename)
        if (opts.rateLimitBps > 0) options["rateLimitBps"] = JsonPrimitive(opts.rateLimitBps)
        if (options.isNotEmpty()) root["options"] = JsonObject(options)
        return JsonObject(root)
    }

    fun magnetParams(magnet: String): JsonObject = JsonObject(
        mapOf("magnet" to JsonPrimitive(magnet))
    )

    private fun JsonObject.string(key: String): String? =
        runCatching { this[key]?.jsonPrimitive?.contentOrNull }.getOrNull()?.takeIf { it.isNotEmpty() }

    private fun JsonObject.long(key: String): Long? =
        runCatching { this[key]?.jsonPrimitive?.longOrNull }.getOrNull()

    private fun JsonObject.boolean(key: String): Boolean? =
        runCatching { this[key]?.jsonPrimitive?.booleanOrNull }.getOrNull()
}
