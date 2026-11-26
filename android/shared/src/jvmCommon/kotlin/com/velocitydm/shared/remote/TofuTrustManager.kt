package com.velocitydm.shared.remote

import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

/**
 * TOFU (trust-on-first-use) pinning of the daemon's self-signed
 * certificate (S8-W1, gate 0018 ATKR-5):
 *
 *  - first connection to an unpinned daemon: the leaf certificate's
 *    SHA-256 fingerprint is ACCEPTED AND STORED. This is the documented
 *    MITM window — an attacker positioned before the first pairing can
 *    pin their own cert. The mitigation is out-of-band comparison:
 *    `velocitydm lan-token` on the desktop prints the same fingerprint
 *    the settings screen stores.
 *  - later connections: a DIFFERENT fingerprint is a hard failure
 *    (SSLException surfaces as an explicit certificate-changed error);
 *    it is never silently re-pinned.
 *  - a manually entered fingerprint (from `velocitydm lan-token`)
 *    pre-pins the daemon and closes the first-connect window.
 *
 * Hostname verification is replaced BY the pin: the certificate is
 * self-signed with informational SANs, identity is the pinned key.
 */
interface TofuStore {
    /** The pinned fingerprint (lowercase hex) or null when unpinned. */
    val pinnedFingerprint: String?

    /** Called once when a daemon is pinned on first use. */
    fun onFirstSeen(fingerprint: String)
}

class TofuTrustManager(private val store: TofuStore) : X509TrustManager {

    override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {
        throw CertificateException("client certificates are not expected")
    }

    override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
        val leaf = chain.firstOrNull() ?: throw CertificateException("empty certificate chain")
        val seen = fingerprint(leaf)
        val pinned = store.pinnedFingerprint
        when {
            pinned == null -> store.onFirstSeen(seen)
            !pinned.equals(seen, ignoreCase = true) -> throw CertificateException(
                "daemon certificate changed (pinned ${pinned.take(16)}…, got ${seen.take(16)}…) — " +
                    "possible man-in-the-middle; re-pair only if you deliberately replaced the desktop certificate"
            )
        }
    }

    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()

    companion object {
        /** SHA-256 of the certificate DER, lowercase hex (64 chars). */
        fun fingerprint(cert: X509Certificate): String =
            MessageDigest.getInstance("SHA-256")
                .digest(cert.encoded)
                .joinToString("") { "%02x".format(it) }

        /** An SSLContext wired to this trust manager (shared by both
         *  OkHttp clients — RPC and SSE). */
        fun sslContext(tm: X509TrustManager): SSLContext {
            val ctx = SSLContext.getInstance("TLS")
            ctx.init(null, arrayOf<TrustManager>(tm), null)
            return ctx
        }
    }
}
