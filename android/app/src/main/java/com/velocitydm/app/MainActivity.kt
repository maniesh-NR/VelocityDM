package com.velocitydm.app

// SPDX-License-Identifier: GPL-3.0-or-later

import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import com.velocitydm.app.engine.UriUtils
import com.velocitydm.app.ui.theme.VelocityDmTheme
import kotlinx.coroutines.launch

/**
 * Single-activity Compose app. Handles the share intent (ACTION_SEND) and
 * the clipboard sniff prompt (an http(s) media/archive/document URL copied
 * while the app is open offers one-tap enqueueing).
 */
class MainActivity : ComponentActivity() {

    private var sharedUrl by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handleIntent(intent)
        setContent {
            VelocityDmTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    val snackbarHostState = remember { SnackbarHostState() }
                    val scope = rememberCoroutineScope()
                    var clipboardCandidate by remember { mutableStateOf<String?>(null) }

                    LaunchedEffect(Unit) {
                        sniffClipboard()?.let { clipboardCandidate = it }
                    }
                    LaunchedEffect(clipboardCandidate) {
                        val url = clipboardCandidate ?: return@LaunchedEffect
                        val result = snackbarHostState.showSnackbar(
                            message = getString(R.string.clipboard_prompt, url),
                            actionLabel = getString(R.string.add),
                            duration = SnackbarDuration.Short,
                        )
                        if (result == SnackbarResult.ActionPerformed) {
                            sharedUrl = url
                            clipboardCandidate = null
                        }
                    }
                    LaunchedEffect(sharedUrl) {
                        sharedUrl?.let { url ->
                            scope.launch {
                                com.velocitydm.app.service.DownloadService.enqueue(
                                    this@MainActivity,
                                    url,
                                )
                            }
                        }
                    }
                    Column(modifier = Modifier.fillMaxSize()) {
                        com.velocitydm.app.ui.screens.VelocityDmApp(prefillUrl = sharedUrl)
                        SnackbarHost(hostState = snackbarHostState)
                    }
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        if (intent?.action == Intent.ACTION_SEND) {
            val text = intent.getStringExtra(Intent.EXTRA_TEXT)
            if (text != null && text.startsWith("http://") || text?.startsWith("https://") == true) {
                sharedUrl = text.trim()
            }
        }
    }

    private fun sniffClipboard(): String? {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val text = clipboard.primaryClip?.getItemAt(0)?.text?.toString()?.trim() ?: return null
        if (!text.startsWith("http://") && !text.startsWith("https://")) return null
        if (text == lastSeenClipboard) return null
        lastSeenClipboard = text
        val looksLikeFile = UriUtils.filenameFromUrl(text) != null ||
            text.substringAfterLast('/').contains('.')
        return if (looksLikeFile) text else null
    }

    companion object {
        var lastSeenClipboard: String? = null
    }
}
