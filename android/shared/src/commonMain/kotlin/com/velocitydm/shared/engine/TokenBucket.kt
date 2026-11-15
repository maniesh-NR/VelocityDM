package com.velocitydm.shared.engine

/**
 * Pure token-bucket wait math (commonMain) for per-download rate
 * limits. The JVM engine owns the clock and the sleeping; this file
 * computes refill/wait so the policy is unit-testable with a virtual
 * clock on every target.
 */

/**
 * One token-bucket computation step.
 *
 * @param tokens token balance before the step (fractional bytes)
 * @param lastRefillMs monotonic timestamp of the last refill
 * @param nowMs current monotonic timestamp
 * @param rateBps limit in bytes/second (<= 0 disables the limiter)
 * @param demand bytes the caller wants to write now
 * @return [TokenBucketStep] with the new balance/timestamp and how
 *         long the caller must wait (0 when the demand fits)
 */
data class TokenBucketStep(
    val tokens: Double,
    val lastRefillMs: Long,
    val waitMs: Long,
)

/**
 * Mirrors the historic bucket behavior: capacity of 256 KiB, fractional
 * refill at [rateBps], wait at most 100 ms per iteration (the caller
 * loops until the demand fits).
 */
fun tokenBucketStep(
    tokens: Double,
    lastRefillMs: Long,
    nowMs: Long,
    rateBps: Long,
    demand: Long,
    burstCap: Double = 256.0 * 1024,
): TokenBucketStep {
    if (rateBps <= 0 || demand <= 0) {
        return TokenBucketStep(tokens, lastRefillMs, 0)
    }
    val filled = tokens + (nowMs - lastRefillMs) * rateBps / 1000.0
    val balance = minOf(filled, burstCap)
    if (balance >= demand) {
        return TokenBucketStep(balance - demand, nowMs, 0)
    }
    val deficit = demand - balance
    val rawWait = (deficit * 1000 / rateBps).toLong()
    return TokenBucketStep(balance, nowMs, rawWait.coerceIn(1, 100))
}
