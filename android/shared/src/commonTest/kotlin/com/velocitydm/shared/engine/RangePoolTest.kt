package com.velocitydm.shared.engine

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class RangePoolTest {

    @Test
    fun leasesExhaustThePoolExactly() {
        val pool = mutableListOf(0L to 999L)
        var leased = 0L
        while (true) {
            val l = leaseInterval(pool, maxLen = 300) ?: break
            leased += l.second - l.first + 1
        }
        assertEquals(1000L, leased)
        assertTrue(poolExhausted(pool))
        assertEquals(0L, poolBytes(pool))
    }

    @Test
    fun leaseIsTailFirst() {
        val pool = mutableListOf(0L to 999L)
        val l = leaseInterval(pool, maxLen = 300)!!
        assertEquals(700L to 999L, l)
        assertEquals(0L to 699L, pool[0])
    }

    @Test
    fun partialLeasesSplitAcrossIntervals() {
        val pool = mutableListOf(0L to 99L, 500L to 599L)
        val total = poolBytes(pool)
        var leased = 0L
        while (true) {
            val l = leaseInterval(pool, maxLen = 60) ?: break
            assertTrue(l.second >= l.first)
            leased += l.second - l.first + 1
        }
        assertEquals(total, leased)
    }

    @Test
    fun emptyPoolReturnsNull() {
        assertNull(leaseInterval(mutableListOf(), 100))
        assertNull(leaseInterval(mutableListOf(5L to 4L), 100)) // exhausted interval
    }

    @Test
    fun releasePutsUnfinishedWorkBack() {
        val pool = mutableListOf(0L to 99L)
        val l = leaseInterval(pool, 100)!!
        releaseInterval(pool, l.copy(first = l.first + 40)) // only 40 bytes fetched
        assertFalse(poolExhausted(pool))
        assertEquals(60L, poolBytes(pool))
    }

    @Test
    fun releaseOfFullyFetchedLeaseIsNoop() {
        val pool = mutableListOf<Interval>()
        releaseInterval(pool, 10L to 9L) // second < first -> nothing pending
        assertTrue(poolExhausted(pool))
    }
}
