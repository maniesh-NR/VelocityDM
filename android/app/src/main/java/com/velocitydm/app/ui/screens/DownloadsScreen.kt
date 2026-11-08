package com.velocitydm.app.ui.screens

// SPDX-License-Identifier: GPL-3.0-or-later

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.velocitydm.app.data.db.DownloadEntity
import com.velocitydm.app.ui.DownloadUiItem
import com.velocitydm.app.ui.DownloadsViewModel
import com.velocitydm.app.util.Formatters

/** Downloads tab: filter chips plus the live progress list. */
@Composable
fun DownloadsScreen(viewModel: DownloadsViewModel) {
    val downloads by viewModel.downloads.collectAsState()
    val live by viewModel.liveProgress.collectAsState()
    var filter by remember { mutableStateOf("all") }

    Column(modifier = Modifier.fillMaxSize().padding(12.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("all" to "All", "running" to "Running", "done" to "Done").forEach { (key, label) ->
                FilterChip(selected = filter == key, onClick = { filter = key }, label = { Text(label) })
            }
        }
        val visible = downloads.filter {
            when (filter) {
                "running" -> it.state == DownloadEntity.STATE_RUNNING || it.state == DownloadEntity.STATE_QUEUED
                "done" -> it.state == DownloadEntity.STATE_COMPLETED
                else -> true
            }
        }
        if (visible.isEmpty()) {
            Text(
                text = "No downloads yet. Add a URL from the Add tab, or share a link into VelocityDM.",
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(top = 24.dp),
            )
        }
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(visible, key = { it.id }) { item ->
                DownloadRow(item, live[item.id], viewModel)
            }
        }
    }
}

@Composable
private fun DownloadRow(item: DownloadUiItem, progress: Pair<Long, Long>?, viewModel: DownloadsViewModel) {
    val done = progress?.first ?: item.downloaded
    val total = progress?.second ?: item.total
    Column(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Text(item.filename, style = MaterialTheme.typography.titleMedium)
        Text(
            "${item.state.uppercase()}  ${Formatters.formatBytes(done)} / ${Formatters.formatBytes(total)}  " +
                Formatters.formatSpeed(speedEstimate(item, done)),
            style = MaterialTheme.typography.labelSmall,
        )
        LinearProgressIndicator(
            progress = { if (total > 0) (done.toFloat() / total).coerceIn(0f, 1f) else 0f },
            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        )
        item.errorMsg?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.labelSmall) }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            when (item.state) {
                DownloadEntity.STATE_RUNNING, DownloadEntity.STATE_QUEUED ->
                    OutlinedButton(onClick = { viewModel.pause(item.id) }) { Text("Pause") }
                DownloadEntity.STATE_PAUSED, DownloadEntity.STATE_FAILED ->
                    Button(onClick = { viewModel.resume(item.id) }) { Text("Resume") }
                else -> {}
            }
            OutlinedButton(onClick = { viewModel.remove(item.id) }) { Text("Remove") }
        }
    }
}

// Live speed arrives through the throttler snapshot; the row-level estimate
// re-derives it from consecutive progress observations.
private val lastObservations = HashMap<Long, Pair<Long, Long>>()
private fun speedEstimate(item: DownloadUiItem, done: Long): Long {
    val now = System.currentTimeMillis()
    val previous = lastObservations[item.id]
    lastObservations[item.id] = done to now
    if (previous == null) return 0
    val dt = (now - previous.second) / 1000.0
    if (dt < 0.5) return 0
    return ((done - previous.first) / dt).toLong().coerceAtLeast(0)
}
