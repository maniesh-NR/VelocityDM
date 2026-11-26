package com.velocitydm.shared.remote

import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.BufferedReader
import java.io.Closeable
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * JSON-RPC 2.0 + SSE client of the desktop daemon's LAN mode (S8-W1):
 * dedicated OkHttp clients (cookie-less, no redirects — a 3xx would
 * move the Authorization header off the daemon), the bearer token on
 * EVERY request via an interceptor (never a query string), TOFU-pinned
 * TLS. The RPC client has call/read timeouts; the SSE client has NO
 * read timeout (an idle daemon is legitimately silent — the gate-0010
 * consumer contract detects death through RPC failures + reconnects).
 */
class RemoteRpc(private val config: RemoteConfig, private val tofu: TofuTrustManager) {

    private val jsonMediaType = "application/json".toMediaType()

    private val client: OkHttpClient = baseBuilder()
        .callTimeout(30, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS)
        .build()

    private val sseClient: OkHttpClient = baseBuilder()
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .build()

    private fun baseBuilder(): OkHttpClient.Builder {
        val ssl = TofuTrustManager.sslContext(tofu)
        return OkHttpClient.Builder()
            .sslSocketFactory(ssl.socketFactory, tofu)
            .hostnameVerifier { _, _ -> true } // identity = the TOFU pin, not the name
            .followRedirects(false)
            .connectTimeout(10, TimeUnit.SECONDS)
            .addInterceptor(authInterceptor())
    }

    private fun authInterceptor() = Interceptor { chain ->
        chain.proceed(
            chain.request().newBuilder()
                .header("Authorization", "Bearer ${config.token}")
                .build()
        )
    }

    /** Convenience probe for pairing UIs (no serialization types leak
     *  into the app module). */
    fun ping(): Result<String> =
        call("daemon.ping", JsonObject(emptyMap())).map { it.toString() }

    /** A synchronous JSON-RPC call. Errors are typed for the UI:
     *  [RpcFailure.Unreachable] (no daemon), [RpcFailure.Rejected]
     *  (401 — wrong token), [RpcFailure.Error] (RPC-level error or
     *  bad payload). */
    fun call(method: String, params: JsonObject): Result<kotlinx.serialization.json.JsonElement> {
        val body = Json.encodeToString(
            JsonObject(
                mapOf(
                    "jsonrpc" to JsonPrimitive("2.0"),
                    "id" to JsonPrimitive(1),
                    "method" to JsonPrimitive(method),
                    "params" to params,
                )
            )
        ).toRequestBody(jsonMediaType)
        val request = Request.Builder()
            .url("${config.baseUrl}/rpc")
            .post(body)
            .build()
        return execute(request) { text ->
            val obj = Json.parseToJsonElement(text).jsonObject
            obj["error"]?.let { err ->
                val msg = (err as? JsonObject)?.get("message").toString()
                return@execute Result.failure(RpcFailure.Error(msg.trim('"')))
            }
            Result.success(obj["result"] ?: kotlinx.serialization.json.JsonNull)
        }
    }

    /** Opens the SSE stream and reads it on the calling thread until
     *  the stream dies; each completed `data:` payload is handed to
     *  [onEvent]. Returns when the stream ends (caller reconnects with
     *  backoff). Throws on connection failure. */
    fun readEvents(onEvent: (String) -> Unit) {
        val parser = SseFrameParser()
        val request = Request.Builder()
            .url("${config.baseUrl}/events")
            .get()
            .build()
        sseClient.newCall(request).execute().use { response ->
            when {
                !response.isSuccessful -> throw IOException("SSE HTTP ${response.code}")
                response.body == null -> throw IOException("empty SSE body")
            }
            val reader = BufferedReader(response.body!!.charStream())
            while (true) {
                val line = reader.readLine() ?: break
                parser.feed(line).forEach(onEvent)
            }
        }
    }

    private fun <T> execute(request: Request, parse: (String) -> Result<T>): Result<T> {
        val response = try {
            client.newCall(request).execute()
        } catch (e: javax.net.ssl.SSLException) {
            return Result.failure(RpcFailure.Rejected("TLS: ${e.message}"))
        } catch (e: IOException) {
            return Result.failure(RpcFailure.Unreachable(e.message ?: "unreachable"))
        }
        response.use { r ->
            val text = r.body?.string() ?: ""
            return when {
                r.code == 401 -> Result.failure(RpcFailure.Rejected("401 unauthorized — check the token"))
                !r.isSuccessful -> Result.failure(RpcFailure.Error("HTTP ${r.code}"))
                else -> runCatching { parse(text) }
                    .getOrElse { Result.failure(RpcFailure.Error("bad response: ${it.message}")) }
            }
        }
    }

    sealed class RpcFailure(message: String) : Exception(message) {
        class Unreachable(message: String) : RpcFailure(message)
        class Rejected(message: String) : RpcFailure(message)
        class Error(message: String) : RpcFailure(message)
    }
}
