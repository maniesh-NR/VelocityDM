package com.velocitydm.shared.engine

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * The `.vdmctl` crash-safe resume control file (commonMain).
 *
 * Written next to the `.part` file while a segmented download runs;
 * read back on resume. Gate 0005 hardened the trust boundary: the
 * control file lives in app-visible storage (on API 26-28 other apps
 * holding legacy storage permissions can tamper with it), so the
 * schema validates strictly and any violation means "restart from
 * zero" rather than feeding attacker-controlled offsets into
 * `RandomAccessFile.seek`.
 */

@Serializable
data class VdmctlInterval(val start: Long, val end: Long)

@Serializable
data class VdmctlFile(
    val url: String,
    val size: Long,
    val etag: String = "",
    val remaining: List<VdmctlInterval> = emptyList(),
)

/** Strict parser: unknown keys, malformed JSON, or invalid intervals -> null. */
object Vdmctl {
    /** Strictest sensible settings: unknown keys are REJECTED. */
    private val json = Json { ignoreUnknownKeys = false }

    /**
     * Parse and validate. `null` means the state is unusable and the
     * download must restart from zero. Validation (gate 0005):
     *   - every interval satisfies `0 <= start <= end < size`
     *   - the total coverage never exceeds `size` (no overlapping
     *     over-claims that would corrupt the done-counter)
     *   - `size > 0`
     */
    fun parse(text: String): VdmctlFile? {
        val f = try {
            json.decodeFromString<VdmctlFile>(text)
        } catch (_: Exception) {
            return null
        }
        if (!isValid(f)) return null
        return f
    }

    fun isValid(f: VdmctlFile): Boolean {
        if (f.size <= 0) return false
        if (f.remaining.isEmpty()) return false
        var covered = 0L
        for ((s, e) in f.remaining) {
            if (s < 0 || e < s || e >= f.size) return false
            covered += e - s + 1
            if (covered > f.size) return false
        }
        return true
    }

    fun encode(f: VdmctlFile): String = json.encodeToString(VdmctlFile.serializer(), f)

    /**
     * Resume policy (gate 0005): stale state must never resume silently.
     *  - matching ETags (both non-empty) -> resumable
     *  - probe ETag absent -> resumable only when the stored size
     *    matches the probe size
     *  - ETag mismatch -> restart
     */
    fun resumableWith(stored: VdmctlFile?, probeSize: Long, probeEtag: String?): Boolean {
        if (stored == null || !isValid(stored)) return false
        if (stored.size != probeSize) return false
        if (probeEtag != null && probeEtag.isNotBlank()) {
            return stored.etag == probeEtag
        }
        // No usable ETag from the server: size match is the only guard.
        return true
    }
}
