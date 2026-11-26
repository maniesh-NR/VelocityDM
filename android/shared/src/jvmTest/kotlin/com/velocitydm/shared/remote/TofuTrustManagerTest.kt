package com.velocitydm.shared.remote

import java.io.ByteArrayInputStream
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import java.security.cert.CertificateException
import javax.net.ssl.SSLException
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.fail

/**
 * TOFU pinning against two fixed self-signed certificates generated
 * once with OpenSSL (prime256v1, CN=test-daemon-1/2). Their DER SHA-256
 * fingerprints are pinned in the assertions, so a change in the
 * fingerprint computation breaks the test loudly.
 */
class TofuTrustManagerTest {

    private val cert1Pem = """
-----BEGIN CERTIFICATE-----
MIIBhDCCASugAwIBAgIUO+i6L8EA6Kxi+ZRl3jPSqN0B48cwCgYIKoZIzj0EAwIw
GDEWMBQGA1UEAwwNdGVzdC1kYWVtb24tMTAeFw0yNjExMjYwNTQzMThaFw0zNjEx
MjMwNTQzMThaMBgxFjAUBgNVBAMMDXRlc3QtZGFlbW9uLTEwWTATBgcqhkjOPQIB
BggqhkjOPQMBBwNCAAR0GVkSUJwe0Gd24Epnv5kHZw8Mtqz4fimYORUod2IMnaUB
8vGUje2X1ojo9evsCDC3MEQoVLJb3Mx/auT4UsB8o1MwUTAdBgNVHQ4EFgQUL6Nd
F/2VAvHKRtX4TW6n4v6eJzcwHwYDVR0jBBgwFoAUL6NdF/2VAvHKRtX4TW6n4v6e
JzcwDwYDVR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNHADBEAiAGduldynNMXd3I
zfopH1ClNHdcCdShnQg96JNxEcur2gIgPkdLgkHpxrwKjZpAmWzzXDcJadX74Bmk
x2nx32cM94o=
-----END CERTIFICATE-----
""".trim()

    private val cert2Pem = """
-----BEGIN CERTIFICATE-----
MIIBhjCCASugAwIBAgIULBuhviz01wsq5l9cheyxvRKHkuQwCgYIKoZIzj0EAwIw
GDEWMBQGA1UEAwwNdGVzdC1kYWVtb24tMjAeFw0yNjExMjYwNTQzMThaFw0zNjEx
MjMwNTQzMThaMBgxFjAUBgNVBAMMDXRlc3QtZGFlbW9uLTIwWTATBgcqhkjOPQIB
BggqhkjOPQMBBwNCAAQoKntvJmfiGi02kPVUyx+JguJxx0PBtj+AGWtLjF8dfH4k
/vQToSuCqqjmWAy14Tc8I8qMO1FHNAAW9MYqJE1xo1MwUTAdBgNVHQ4EFgQUTtGw
G9mvLCtjYaP2hSo6XQZ2YHIwHwYDVR0jBBgwFoAUTtGwG9mvLCtjYaP2hSo6XQZ2
YHIwDwYDVR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNJADBGAiEAhuZUO7j9HeJ1
XM8cv7albtIYK2Y3aVzr7IT+5+979XECIQC6fdFXUOo6eWZwemyEwLVSKHk6+WrR
FCRZUkCpZCqhWA==
-----END CERTIFICATE-----
""".trim()

    private fun load(pem: String): X509Certificate =
        CertificateFactory.getInstance("X.509")
            .generateCertificate(ByteArrayInputStream(pem.toByteArray()))
            as X509Certificate

    private class MemStore : TofuStore {
        override var pinnedFingerprint: String? = null
        var firstSeenCount = 0
        override fun onFirstSeen(fingerprint: String) {
            pinnedFingerprint = fingerprint
            firstSeenCount++
        }
    }

    @Test
    fun fingerprintIsTheDerSha256() {
        // openssl x509 -outform DER | sha256sum, computed when the
        // fixture was generated:
        assertEquals(
            "b4736ef4862f613beda0abc42aae52340ba9f5b029cd9ad7aa61e798ce1fc01a",
            TofuTrustManager.fingerprint(load(cert1Pem)),
        )
        assertEquals(
            "a57aa70691a5d0a12215ff7d58e13fe2b854f240bf6ec3daa24e5e24250d5da3",
            TofuTrustManager.fingerprint(load(cert2Pem)),
        )
    }

    @Test
    fun firstConnectionPinsAndLaterSameCertPasses() {
        val store = MemStore()
        val tm = TofuTrustManager(store)
        val chain = arrayOf(load(cert1Pem))
        tm.checkServerTrusted(chain, "ECDHE_ECDSA") // first use: accept + pin
        assertEquals(
            "b4736ef4862f613beda0abc42aae52340ba9f5b029cd9ad7aa61e798ce1fc01a",
            store.pinnedFingerprint,
        )
        assertEquals(1, store.firstSeenCount)
        tm.checkServerTrusted(chain, "ECDHE_ECDSA") // same cert: pass, no re-pin
        assertEquals(1, store.firstSeenCount)
    }

    @Test
    fun differentCertificateIsAHardFailureNeverRePinned() {
        val store = MemStore()
        val tm = TofuTrustManager(store)
        tm.checkServerTrusted(arrayOf(load(cert1Pem)), "ECDHE_ECDSA")
        val e = assertFailsWith<CertificateException> {
            tm.checkServerTrusted(arrayOf(load(cert2Pem)), "ECDHE_ECDSA")
        }
        // the error names both fingerprints (prefix) so the user can act
        assert(e.message!!.contains("certificate changed"))
        // the pin was NOT updated — a retry keeps failing
        assertEquals(
            "b4736ef4862f613beda0abc42aae52340ba9f5b029cd9ad7aa61e798ce1fc01a",
            store.pinnedFingerprint,
        )
    }

    @Test
    fun manualPrePinClosesTheFirstConnectWindow() {
        val store = object : TofuStore {
            override val pinnedFingerprint =
                "b4736ef4862f613beda0abc42aae52340ba9f5b029cd9ad7aa61e798ce1fc01a"
            override fun onFirstSeen(fingerprint: String) = fail("must not re-pin a pre-pinned daemon")
        }
        val tm = TofuTrustManager(store)
        tm.checkServerTrusted(arrayOf(load(cert1Pem)), "ECDHE_ECDSA") // passes, no TOFU accept
        assertFailsWith<CertificateException> {
            tm.checkServerTrusted(arrayOf(load(cert2Pem)), "ECDHE_ECDSA")
        }
    }

    @Test
    fun clientChainsAndEmptyChainsRefused() {
        val tm = TofuTrustManager(MemStore())
        assertFailsWith<CertificateException> {
            tm.checkClientTrusted(arrayOf(load(cert1Pem)), "ECDHE_ECDSA")
        }
        assertFailsWith<CertificateException> {
            tm.checkServerTrusted(emptyArray(), "ECDHE_ECDSA")
        }
    }
}
