package com.velocitydm.android.data

import android.content.Context
import com.velocitydm.shared.remote.RemoteConfig
import com.velocitydm.shared.remote.TofuStore

/**
 * Remote-mode settings (S8-W1): plain SharedPreferences — app-private
 * storage, no new dependency (the keystore-encrypted variant is a
 * recorded W1 non-goal with its caveat documented in the spec). The
 * token is entered once during pairing and read on every connect.
 */
object RemotePrefs {
    private const val FILE = "vdm_remote"

    private const val KEY_ENABLED = "enabled"
    private const val KEY_HOST = "host"
    private const val KEY_PORT = "port"
    private const val KEY_TOKEN = "token"
    private const val KEY_FINGERPRINT = "fingerprint"

    data class Settings(
        val enabled: Boolean,
        val host: String,
        val port: Int,
        val token: String,
        /** Manually entered pin (empty = TOFU on first connect). */
        val fingerprint: String,
    ) {
        fun toConfig(): RemoteConfig? {
            val cfg = RemoteConfig(
                host = host.trim(),
                port = port,
                token = token.trim().lowercase(),
                fingerprint = fingerprint.trim().takeIf { it.isNotEmpty() },
            )
            return if (cfg.isValid()) cfg else null
        }

        val invalidReason: String?
            get() = RemoteConfig(
                host = host.trim(),
                port = port,
                token = token.trim().lowercase(),
                fingerprint = null,
            ).reason
    }

    fun read(ctx: Context): Settings {
        val p = ctx.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        return Settings(
            enabled = p.getBoolean(KEY_ENABLED, false),
            host = p.getString(KEY_HOST, "") ?: "",
            port = p.getInt(KEY_PORT, 36990),
            token = p.getString(KEY_TOKEN, "") ?: "",
            fingerprint = p.getString(KEY_FINGERPRINT, "") ?: "",
        )
    }

    fun write(ctx: Context, s: Settings) {
        ctx.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit()
            .putBoolean(KEY_ENABLED, s.enabled)
            .putString(KEY_HOST, s.host.trim())
            .putInt(KEY_PORT, s.port)
            .putString(KEY_TOKEN, s.token.trim().lowercase())
            .putString(KEY_FINGERPRINT, s.fingerprint.trim().lowercase())
            .apply()
    }

    /** The TOFU store view over the same file (the pin updates on
     *  first connect; a manual fingerprint pre-seeds it). */
    fun tofuStore(ctx: Context): TofuStore = object : TofuStore {
        private val p = ctx.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        override val pinnedFingerprint: String?
            get() = p.getString(KEY_FINGERPRINT, null)?.takeIf { it.isNotEmpty() }
        override fun onFirstSeen(fingerprint: String) {
            p.edit().putString(KEY_FINGERPRINT, fingerprint).apply()
        }
    }
}
