package com.velocitydm.app.ui.screens

// SPDX-License-Identifier: GPL-3.0-or-later

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.velocitydm.app.ui.DownloadsViewModel
import kotlinx.coroutines.delay

/** Add tab: URL entry with engine options (parts, checksum, wifi-only). */
@Composable
fun AddScreen(viewModel: DownloadsViewModel, prefillUrl: String?) {
    var url by remember { mutableStateOf("") }
    var filename by remember { mutableStateOf("") }
    var checksum by remember { mutableStateOf("") }
    var wifiOnly by remember { mutableStateOf(false) }
    var parts by remember { mutableFloatStateOf(8f) }
    var message by remember { mutableStateOf<String?>(null) }
    var lastPrefill by remember { mutableStateOf<String?>(null) }

    // Share-intent and clipboard-sniff URLs land here exactly once.
    LaunchedEffect(prefillUrl) {
        if (prefillUrl != null && prefillUrl != lastPrefill) {
            url = prefillUrl
            lastPrefill = prefillUrl
        }
    }

    Column(modifier = Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        OutlinedTextField(
            value = url,
            onValueChange = { url = it },
            label = { Text("URL") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = filename,
            onValueChange = { filename = it },
            label = { Text("Filename (optional override)") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = checksum,
            onValueChange = { checksum = it },
            label = { Text("Checksum, e.g. sha256:... (optional)") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Text("Parallel parts: ${parts.toInt()}", style = MaterialTheme.typography.bodyMedium)
        Slider(value = parts, onValueChange = { parts = it }, valueRange = 1f..16f, steps = 14)
        androidx.compose.foundation.layout.Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Switch(checked = wifiOnly, onCheckedChange = { wifiOnly = it })
            Text("Download on Wi-Fi only", modifier = Modifier.padding(top = 12.dp))
        }
        Button(
            onClick = {
                viewModel.enqueue(url, filename, checksum, wifiOnly) { error ->
                    if (error != null) {
                        message = error
                    } else {
                        message = "Queued: " + url.substringAfterLast('/')
                        url = ""
                        filename = ""
                        checksum = ""
                    }
                }
            },
            enabled = url.isNotBlank(),
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Add download") }
        message?.let {
            Text(it, style = MaterialTheme.typography.labelSmall)
            LaunchedEffect(it) {
                delay(4000)
                message = null
            }
        }
    }
}
