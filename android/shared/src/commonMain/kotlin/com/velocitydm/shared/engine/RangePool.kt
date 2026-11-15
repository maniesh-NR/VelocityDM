package com.velocitydm.shared.engine

/**
 * Pure interval-leasing math for segmented downloads (commonMain).
 *
 * The work-stealing range pool from the Kotlin engine (mirroring the
 * Rust engine's model): a mutable list of `[start, end]` byte
 * intervals still to fetch; workers lease chunks off the tail of any
 * non-empty interval and release unfinished leases back on failure or
 * pause. Callers own the synchronization (the JVM engine wraps these
 * calls in `synchronized` blocks); this file stays pure so the math is
 * unit-testable on every target.
 */

/** One byte-range lease: inclusive `[start, end]`. */
typealias Interval = Pair<Long, Long>

/**
 * Lease up to [maxLen] bytes from [intervals] (mutated in place).
 * Returns the leased inclusive interval, or `null` when the pool is
 * exhausted. Leases are taken from the tail of the first non-empty
 * interval (tail-first keeps resumed files compact and mirrors the
 * engine's historic behavior).
 */
fun leaseInterval(intervals: MutableList<Interval>, maxLen: Long): Interval? {
    val idx = intervals.indexOfFirst { it.second - it.first + 1 > 0 }
    if (idx < 0) return null
    val (s, e) = intervals[idx]
    val leaseLen = minOf(maxLen, e - s + 1)
    val ls = e - leaseLen + 1
    intervals[idx] = s to ls - 1
    return ls to e
}

/** Return an unfinished (or partially fetched) lease to the pool. */
fun releaseInterval(intervals: MutableList<Interval>, lease: Interval) {
    if (lease.second >= lease.first) intervals.add(lease)
}

/**
 * True when every interval is exhausted: nothing left to download.
 */
fun poolExhausted(intervals: List<Interval>): Boolean =
    intervals.none { it.second >= it.first }

/** Total bytes covered by the intervals (sum of inclusive lengths). */
fun poolBytes(intervals: List<Interval>): Long =
    intervals.sumOf { (it.second - it.first + 1).coerceAtLeast(0) }
