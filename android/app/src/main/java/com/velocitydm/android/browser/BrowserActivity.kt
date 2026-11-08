package com.velocitydm.android.browser

import android.annotation.SuppressLint
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView

/**
 * Built-in sniffer browser (1DM-style): watches page traffic for media
 * (m3u8/mpd/video files) and lists everything found for one-tap download.
 */
@SuppressLint("SetJavaScriptEnabled")
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BrowserScreen(
    onClose: () -> Unit,
    onDownload: (url: String, filename: String) -> Unit,
) {
    var address by remember { mutableStateOf("https://example.com") }
    var sniffed by remember { mutableStateOf(listOf<SniffedMedia>()) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    OutlinedTextField(
                        value = address,
                        onValueChange = { address = it },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                        textStyle = MaterialTheme.typography.bodyMedium,
                    )
                },
                navigationIcon = {
                    TextButton(onClick = onClose) { Text("Close") }
                },
                actions = {
                    TextButton(onClick = { sniffed = emptyList() }) { Text("Clear") }
                },
            )
        },
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize()) {
            if (sniffed.isNotEmpty()) {
                Text(
                    "${sniffed.size} media found",
                    color = MaterialTheme.colorScheme.primary,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                )
                sniffed.take(6).forEach { m ->
                    Surface(
                        color = MaterialTheme.colorScheme.surfaceContainer,
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 8.dp, vertical = 2.dp),
                        shape = MaterialTheme.shapes.medium,
                    ) {
                        Row(Modifier.padding(10.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                            Column(Modifier.weight(1f)) {
                                Text(m.title, maxLines = 1, style = MaterialTheme.typography.bodySmall)
                                Text(m.url, maxLines = 1, fontSize = 10.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            TextButton(onClick = { onDownload(m.url, m.title) }) { Text("Get") }
                        }
                    }
                }
                HorizontalDivider(Modifier.padding(vertical = 4.dp))
            }
            AndroidView(
                factory = { ctx ->
                    WebView(ctx).apply {
                        settings.javaScriptEnabled = true
                        settings.mediaPlaybackRequiresUserGesture = false
                        webViewClient = SniffWebViewClient { media ->
                            sniffed = (listOf(media) + sniffed).distinctBy { it.url }.take(20)
                        }
                        loadUrl(address)
                    }
                },
                update = { wv -> if (wv.url != address && address.startsWith("http")) wv.loadUrl(address) },
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}

data class SniffedMedia(val url: String, val title: String, val kind: String)

private class SniffWebViewClient(
    private val onSniff: (SniffedMedia) -> Unit,
) : WebViewClient() {
    private val seen = mutableSetOf<String>()

    override fun shouldInterceptRequest(
        view: WebView,
        request: WebResourceRequest,
    ): WebResourceResponse? {
        val url = request.url.toString()
        val mime = request.requestHeaders["Accept"]?.lowercase() ?: ""
        classify(url)?.let { kind ->
            if (seen.add(url)) {
                val title = url.substringAfterLast('/').substringBefore('?').ifBlank { "media" }
                onSniff(SniffedMedia(url, title, kind))
            }
        }
        return super.shouldInterceptRequest(view, request)
    }

    private fun classify(url: String): String? {
        val u = url.lowercase()
        return when {
            u.contains(".m3u8") -> "hls"
            u.contains(".mpd") -> "dash"
            Regex("\\.(mp4|webm|mkv|mov|m4v)(\\?|$)").containsMatchIn(u) -> "video"
            Regex("\\.(mp3|m4a|flac|opus)(\\?|$)").containsMatchIn(u) -> "audio"
            else -> null
        }
    }
}
