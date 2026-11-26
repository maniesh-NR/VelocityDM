package com.velocitydm.shared.remote

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import kotlin.test.fail

/**
 * LIVE integration witness (S8-W1): the real Kotlin client stack —
 * OkHttp + TOFU trust manager + parser — against a REAL Rust daemon
 * built with the `lan` feature.
 *
 * Runs ONLY when the environment points at a running daemon:
 *
 *   VDM_LAN_TEST_ADDR=0.0.0.0-ish-host:port
 *   VDM_LAN_SECRETS=<the daemon's secrets file>   (token + fingerprint)
 *
 * CI never sets these (the android-frontend job has no Rust daemon) —
 * the test is skipped there; it is a manual/runbook witness.
 */
class LanDaemonLiveTest {

    private val addr: String? = System.getenv("VDM_LAN_TEST_ADDR")
    private val secretsPath: String? = System.getenv("VDM_LAN_SECRETS")

    private fun skip(): Boolean = addr == null || secretsPath == null

    private class MemStore : TofuStore {
        override var pinnedFingerprint: String? = null
        override fun onFirstSeen(fingerprint: String) {
            pinnedFingerprint = fingerprint
        }
    }

    @Test
    fun pingListSseAndWrongTokenAgainstTheRealDaemon() {
        if (skip()) return
        val addr = this.addr ?: return
        val secretsFile = this.secretsPath ?: return
        val secrets = Json.parseToJsonElement(java.io.File(secretsFile).readText()).jsonObject
        val token = secrets["token"]!!.jsonPrimitive.content
        val fingerprint = secrets["fingerprint"]!!.jsonPrimitive.content
        val host = addr.substringBeforeLast(':')
        val port = addr.substringAfterLast(':').toInt()

        // 1. wrong token is rejected with the distinct 401 failure
        val wrong = RemoteRpc(
            RemoteConfig(host = host, port = port, token = "f".repeat(64)),
            TofuTrustManager(MemStore()),
        )
        val rejected = wrong.ping().exceptionOrNull()
        assertNotNull(rejected, "wrong token must fail")
        assertTrue(
            rejected is RemoteRpc.RpcFailure.Rejected,
            "expected Rejected(401), got $rejected",
        )

        // 2. correct token: ping + list over TOFU-pinned TLS; the pin
        //    captured on first use must equal the daemon's fingerprint
        val store = MemStore()
        val rpc = RemoteRpc(
            RemoteConfig(host = host, port = port, token = token),
            TofuTrustManager(store),
        )
        assertEquals("\"pong\"", rpc.ping().getOrThrow())
        val list = rpc.call("downloads.list", kotlinx.serialization.json.JsonObject(emptyMap()))
            .getOrThrow()
        assertTrue(list.toString().startsWith("["), "list is an array")
        assertEquals(fingerprint, store.pinnedFingerprint, "TOFU pin equals the daemon fingerprint")

        // 3. the SSE stream delivers a parseable frame header over TLS
        val frames = mutableListOf<String>()
        val reader = Thread {
            runCatching {
                rpc.readEvents { frames.add(it) }
            }
        }
        reader.start()
        val deadline = System.currentTimeMillis() + 5000
        while (frames.isEmpty() && System.currentTimeMillis() < deadline) Thread.sleep(50)
        if (frames.isEmpty()) fail("no SSE frame within 5s")
        assertNotNull(RemoteMapper.parseEvent(frames.first()), "frame parses: ${frames.first()}")
    }
}
