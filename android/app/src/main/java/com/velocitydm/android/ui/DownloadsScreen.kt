package com.velocitydm.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.velocitydm.android.data.DownloadsViewModel
import com.velocitydm.android.engine.TaskState
import java.io.File
import java.util.Locale

fun fmtBytes(b: Long): String {
    if (b < 1024) return "$b B"
    val units = listOf("KiB", "MiB", "GiB")
    var v = b.toDouble() / 1024
    var i = 0
    while (v >= 1024 && i < 2) { v /= 1024; i++ }
    return String.format(Locale.US, "%.1f %s", v, units[i])
}

fun fmtSpeed(bps: Long): String = if (bps > 0) "${fmtBytes(bps)}/s" else "-"

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DownloadsScreen(vm: DownloadsViewModel, incomingUrl: String?) {
    val tasks by vm.tasks.collectAsState()
    var showAdd by remember { mutableStateOf(false) }
    var showBrowser by remember { mutableStateOf(false) }
    var pendingUrl by remember { mutableStateOf<String?>(incomingUrl) }

    LaunchedEffect(pendingUrl) {
        pendingUrl?.let {
            vm.add(it)
            pendingUrl = null
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("VelocityDM", color = MaterialTheme.colorScheme.primary) },
                actions = {
                    TextButton(onClick = { showAdd = true }) { Text("Add") }
                    TextButton(onClick = { showBrowser = true }) { Text("Browser") }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            )
        },
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize()) {
            if (tasks.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Text(
                        "No downloads yet.\nUse Add, the built-in browser, or share\na link from another app.",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        fontSize = 14.sp,
                    )
                }
            } else {
                LazyColumn(
                    Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(8.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    items(tasks, key = { it.id }) { t -> TaskRow(t, onPause = { vm.pause(t.id) }, onResume = { vm.resume(t.id) }, onRemove = { vm.remove(t.id) }) }
                }
            }
        }
    }

    if (showAdd) {
        AddDialog(onDismiss = { showAdd = false }, onAdd = { url, name, limit ->
            vm.add(url, name.ifBlank { null }, limit)
            showAdd = false
        })
    }
    if (showBrowser) {
        com.velocitydm.android.browser.BrowserScreen(
            onClose = { showBrowser = false },
            onDownload = { url, name -> vm.add(url, name) },
        )
    }
}

@Composable
fun TaskRow(t: TaskState, onPause: () -> Unit, onResume: () -> Unit, onRemove: () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainer
        ),
    ) {
        Column(Modifier.padding(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    t.filename,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                    style = MaterialTypographyOf().titleMedium,
                )
                Text(
                    t.state,
                    color = when (t.state) {
                        "completed" -> MaterialTheme.colorScheme.tertiary
                        "failed" -> MaterialTheme.colorScheme.error
                        "downloading" -> MaterialTheme.colorScheme.primary
                        else -> MaterialTheme.colorScheme.onSurfaceVariant
                    },
                    fontSize = 11.sp,
                )
            }
            Spacer(Modifier.height(6.dp))
            val frac = if (t.size > 0) (t.done.toFloat() / t.size).coerceIn(0f, 1f) else 0f
            LinearProgressIndicator(
                progress = { frac },
                modifier = Modifier.fillMaxWidth().height(6.dp),
                color = MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.height(6.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "${fmtBytes(t.done)} / ${if (t.size > 0) fmtBytes(t.size) else "?"}  ${fmtSpeed(t.speed)}",
                    fontSize = 11.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                when (t.state) {
                    "downloading", "probing", "queued" -> TextButton(onClick = onPause) { Text("Pause") }
                    "paused", "failed" -> TextButton(onClick = onResume) { Text("Resume") }
                }
                TextButton(onClick = onRemove) { Text("Remove") }
            }
            t.error?.let {
                Text(it, color = MaterialTheme.colorScheme.error, fontSize = 11.sp)
            }
        }
    }
}

@Composable
private fun MaterialTypographyOf() = MaterialTheme.typography

@Composable
fun AddDialog(onDismiss: () -> Unit, onAdd: (url: String, filename: String, limitBps: Long) -> Unit) {
    var url by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var limit by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("New download") },
        text = {
            Column {
                OutlinedTextField(
                    value = url, onValueChange = { url = it },
                    label = { Text("URL (http/https)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = name, onValueChange = { name = it },
                    label = { Text("File name (optional)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = limit, onValueChange = { limit = it },
                    label = { Text("Speed limit, e.g. 2MB (optional)") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Ascii),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    val bps = parseLimit(limit)
                    onAdd(url.trim(), name.trim(), bps)
                },
                enabled = url.startsWith("http"),
            ) { Text("Add") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

fun parseLimit(s: String): Long {
    val t = s.trim().lowercase()
    if (t.isEmpty()) return 0
    val m = Regex("([0-9.]+)\\s*(k|m|g|ki|mi|gi)?b?").find(t) ?: return 0
    val n = m.groupValues[1].toDoubleOrNull() ?: return 0
    val mult = when (m.groupValues[2]) {
        "k", "ki" -> 1024.0
        "m", "mi" -> 1024.0 * 1024
        "g", "gi" -> 1024.0 * 1024 * 1024
        else -> 1.0
    }
    return (n * mult).toLong()
}

@Suppress("unused")
private fun unusedDir(f: File) { }
