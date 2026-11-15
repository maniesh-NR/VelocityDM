package com.velocitydm.shared.md3

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The Rust<->Kotlin drift tripwire (gate 0005).
 *
 * `md3-tokens-baseline.json` is generated from the CANONICAL Rust
 * engine by scripts/gen-md3-baseline.py. This test asserts the Kotlin
 * port reproduces every field of every named seed, both schemes —
 * byte-for-byte. Any mismatch means the two engines drifted; fix the
 * drift (or regenerate the baseline after a deliberate change) in the
 * same commit.
 *
 * Lives in jvmTest (not commonTest) because it reads the checked-in
 * resource via the JVM classloader.
 */
class Md3GoldenTest {

    @Serializable
    data class BaselineSeed(
        val seedHex: String,
        val name: String,
        val light: Md3Scheme,
        val dark: Md3Scheme,
    )

    @Serializable
    data class BaselineDoc(val seeds: List<BaselineSeed>)

    private fun baseline(): BaselineDoc {
        val stream = javaClass.classLoader!!.getResourceAsStream("md3-tokens-baseline.json")
            ?: error("md3-tokens-baseline.json not on the test classpath")
        val text = stream.use { it.readBytes().decodeToString() }
        val json = Json { ignoreUnknownKeys = true } // "provenance" field
        return json.decodeFromString(text)
    }

    @Test
    fun kotlinPortMatchesRustBaselineExactly() {
        val doc = baseline()
        assertTrue(doc.seeds.size >= 8, "expected the full named-seed set, got ${doc.seeds.size}")
        var checked = 0
        for (seed in doc.seeds) {
            val hex = Md3Scheme.parseHex(seed.seedHex)
                ?: error("unparseable seed hex ${seed.seedHex}")
            for (dark in listOf(false, true)) {
                val expected = if (dark) seed.dark else seed.light
                val actual = Md3Scheme.fromSeedHex(hex, dark)
                assertEquals(expected, actual, "seed ${seed.seedHex} (${seed.name}) ${if (dark) "dark" else "light"}")
                checked++
            }
        }
        assertTrue(checked >= 16, "checked $checked schemes")
    }

    @Test
    fun defaultSeedIsTheRustDefault() {
        val doc = baseline()
        val first = doc.seeds.first()
        assertEquals("4156A4", first.seedHex)
        assertEquals(MD3_DEFAULT_SEED, 0x4156A4)
        // The pre-12 Android static scheme uses this seed (plan S12-e).
        assertEquals(
            Md3Scheme.fromSeedHex(MD3_DEFAULT_SEED, dark = false),
            Md3Scheme.fromSeedHex(Md3Scheme.parseHex(first.seedHex)!!, dark = false),
        )
    }
}
