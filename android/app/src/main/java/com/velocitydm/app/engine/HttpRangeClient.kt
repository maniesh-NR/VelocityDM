package com.velocitydm.app.engine

// SPDX-License-Identifier: GPL-3.0-or-later

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import java.util.concurrent.TimeUnit

data class Probe(
    val total: Long,
    val acceptRanges: Boolean,
    val etag: String?,
    val filename: String?,
    val finalUrl: String,
    val contentType: String?,
)

/** OkHttp-based range-aware HTTP client used by the download engine. */
class HttpRangeClient {
    val http: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    fun probe(url: String): Probe {
        var current = url
        repeat(5) {
            val response = http.newCall(headRequest(current)).execute()
            response.use { resp ->
                if (resp.isRedirect) {
                    val location = resp.header("Location")
                    if (location != null) {
                        current = resolve(current, location)
                        return@repeat
                    }
                }
                val ranges = resp.header("Accept-Ranges")?.contains("bytes") == true
                val total = resp.header("Content-Length")?.toLongOrNull() ?: -1L
                return Probe(
                    total = total,
                    acceptRanges = ranges,
                    etag = resp.header("ETag"),
                    filename = resp.header("Content-Disposition")?.let { UriUtils.filenameFromDisposition(it) },
                    finalUrl = current,
                    contentType = resp.header("Content-Type"),
                )
            }
        }
        // Fall back to a 1-byte ranged GET when HEAD misbehaves.
        val response = http.newCall(rangeRequest(current, 0, 0)).execute()
        response.use { resp ->
            val total = resp.header("Content-Range")
                ?.substringAfterLast('/')?.toLongOrNull() ?: -1L
            return Probe(
                total = total,
                acceptRanges = resp.code == 206,
                etag = resp.header("ETag"),
                filename = resp.header("Content-Disposition")?.let { UriUtils.filenameFromDisposition(it) },
                finalUrl = current,
                contentType = resp.header("Content-Type"),
            )
        }
    }

    /** Opens a streaming ranged GET; caller consumes resp.body.source(). */
    fun open(url: String, rangeStart: Long, rangeEnd: Long?): okhttp3.Response {
        val builder = Request.Builder().url(url).header("Accept-Encoding", "identity")
        builder.header(
            "Range",
            if (rangeEnd != null) "bytes=$rangeStart-$rangeEnd" else "bytes=$rangeStart-",
        )
        return http.newCall(builder.build()).execute()
    }

    private fun headRequest(url: String): Request =
        Request.Builder().url(url).head().header("Accept-Encoding", "identity").build()

    private fun rangeRequest(url: String, start: Long, end: Long): Request =
        Request.Builder().url(url).header("Range", "bytes=$start-$end").build()

    private fun resolve(base: String, location: String): String =
        location.toHttpUrlOrNull()?.toString()
            ?: (base.toHttpUrlOrNull()?.resolve(location)?.toString() ?: location)
}

/** Filename helpers shared with the UI layer. */
object UriUtils {
    fun filenameFromUrl(url: String): String? = try {
        val uri = java.net.URI(url)
        val path = uri.path ?: return null
        val last = path.substringAfterLast('/', "")
        if (last.isEmpty()) null else sanitize(last, requireExtension = true)
    } catch (e: Exception) {
        null
    }

    fun filenameFromDisposition(header: String): String? {
        var star: String? = null
        var plain: String? = null
        for (part in header.split(';')) {
            val idx = part.indexOf('=')
            if (idx <= 0) continue
            val key = part.substring(0, idx).trim().lowercase()
            val value = part.substring(idx + 1).trim()
            when (key) {
                "filename*" -> star = star ?: decodeRfc5987(value)
                "filename" -> plain = plain ?: value.trim('"')
            }
        }
        val raw = star ?: plain ?: return null
        return sanitize(raw, requireExtension = false)
    }

    private fun decodeRfc5987(value: String): String? = try {
        val encoded = value.substringAfter('\'', "").substringAfter('\'', "")
        if (encoded.isEmpty()) null else java.net.URLDecoder.decode(encoded, "UTF-8")
    } catch (e: Exception) {
        null
    }

    fun sanitize(raw: String, requireExtension: Boolean): String? {
        val cleaned = buildString {
            for (c in raw) {
                when (c) {
                    '/', '\\', ':', '*', '?', '"', '<', '>', '|' -> append('_')
                    else -> if (!c.isISOControl()) append(c) else append('_')
                }
            }
        }.trim()
        if (cleaned.isEmpty() || cleaned == "." || cleaned == "..") return null
        val trimmed = if (cleaned.length > 180) {
            val dot = cleaned.lastIndexOf('.')
            if (dot > 0 && cleaned.length - dot < 180) {
                cleaned.take(180 - (cleaned.length - dot)) + cleaned.substring(dot)
            } else {
                cleaned.take(180)
            }
        } else {
            cleaned
        }
        if (requireExtension && !trimmed.contains('.')) return null
        return trimmed.ifEmpty { null }
    }
}

/** Small suspend wrapper so engine code can probe off the main thread. */
suspend fun HttpRangeClient.probeSuspend(url: String): Probe = withContext(Dispatchers.IO) { probe(url) }
