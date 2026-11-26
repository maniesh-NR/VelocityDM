package com.velocitydm.shared.remote

/**
 * Remote-daemon connection settings (S8-W1). Persistence is the app's
 * job (SharedPreferences); this is the pure value the shared module
 * consumes. `fingerprint` is the optional manually-entered TOFU pin
 * (SHA-256 of the daemon certificate DER, lowercase hex — the value
 * `velocitydm lan-token` prints on the desktop).
 */
data class RemoteConfig(
    val host: String,
    val port: Int = 36990,
    val token: String,
    val fingerprint: String? = null,
) {
    /** https only — the manifest keeps usesCleartextTraffic for local
     *  engine dev URLs, so remote mode enforces TLS in code. */
    val baseUrl: String
        get() {
            require(isValid()) { "invalid remote config: $reason" }
            return "https://$host:$port"
        }

    fun isValid(): Boolean = reason == null

    val reason: String?
        get() = when {
            host.isBlank() -> "host is empty"
            host.startsWith("http://") || host.contains("://") -> "host must be a bare host name or IP, not a URL"
            port !in 1..65535 -> "port out of range"
            token.length != 64 -> "token must be 64 hex characters (velocitydm lan-token prints it)"
            !token.all { it.isDigit() || it in 'a'..'f' } -> "token must be lowercase hex"
            else -> null
        }
}
