package com.velocitydm.shared.remote

/**
 * Pure SSE frame-splitter (S8-W1): consumes raw text LINES as they are
 * read from the stream and yields completed `data:` payloads. The
 * daemon's wire shape is exactly `data: <json>\n\n` (one data line per
 * frame, no heartbeats — verified by scripts/sse-probe-transcript.txt),
 * but the parser follows the SSE rules for the shapes it may meet:
 * multi-line `data:` joins with \n, non-data fields are ignored, a
 * blank line dispatches, a 1 MiB buffer cap DROPS the oversized frame
 * (memory-exhaustion defense, gate-0010 parity).
 *
 * Line endings: the daemon writes \n; \r is tolerated and stripped
 * (lenient), including a lone \r as a line terminator.
 */
class SseFrameParser(private val maxBytes: Int = 1024 * 1024) {

    private val data = StringBuilder()
    private var bytes = 0
    private var dropped = 0

    /** Frames dropped for exceeding [maxBytes] (observability). */
    fun droppedFrames(): Int = dropped

    /** Feed one raw line (without its terminator). Returns the
     *  completed payloads (0 or 1 for the daemon's shape). */
    fun feed(line: String): List<String> {
        val l = line.trimEnd('\r')
        if (l.isEmpty()) {
            val out = dispatch()
            return if (out == null) emptyList() else listOf(out)
        }
        if (l.startsWith(":")) return emptyList() // SSE comment/keep-alive
        if (l.startsWith("data:")) {
            bytes += l.length
            if (bytes > maxBytes) {
                // drop the oversized frame entirely and resync at the
                // next dispatch boundary
                dropped++
                data.setLength(0)
                bytes = 0
                pendingDrop = true
                return emptyList()
            }
            if (data.isNotEmpty()) data.append('\n')
            data.append(l.removePrefix("data:").trimStart(' '))
        }
        // other fields (event:, id:, retry:) are ignored — the daemon
        // never sends them
        return emptyList()
    }

    private var pendingDrop = false

    private fun dispatch(): String? {
        bytes = 0
        if (pendingDrop) {
            pendingDrop = false
            data.setLength(0)
            return null
        }
        if (data.isEmpty()) return null
        val out = data.toString()
        data.setLength(0)
        return out
    }
}
