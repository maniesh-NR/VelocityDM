package com.velocitydm.app.ui.screens

// SPDX-License-Identifier: GPL-3.0-or-later

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.velocitydm.app.data.AppSettings
import com.velocitydm.app.ui.AppUiSettings
import com.velocitydm.app.ui.DownloadsViewModel

/** Settings tab: storage, engine defaults, refresh rate, theme. */
@Composable
fun SettingsScreen(viewModel: DownloadsViewModel = viewModel()) {
    val settings by viewModel.settingsUi.collectAsState()
    var speedText by remember(settings.speedLimitBps) { mutableStateOf(settings.speedLimitBps.toString()) }
    var parts by remember(settings.defaultParts) { mutableFloatStateOf(settings.defaultParts.toFloat()) }

    Column(modifier = Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("Settings", style = MaterialTheme.typography.titleLarge)

        Text("Download directory", style = MaterialTheme.typography.titleMedium)
        Text(settings.downloadDir, style = MaterialTheme.typography.bodyMedium)

        Text("Default parallel parts: ${parts.toInt()}", style = MaterialTheme.typography.titleMedium)
        Slider(
            value = parts,
            onValueChange = { parts = it },
            valueRange = 1f..16f,
            steps = 14,
            onValueChangeFinished = { viewModel.setDefaultParts(parts.toInt()) },
        )

        Text("Global speed limit (bytes/sec, 0 = unlimited)", style = MaterialTheme.typography.titleMedium)
        OutlinedTextField(
            value = speedText,
            onValueChange = { speedText = it.filter { c -> c.isDigit() } },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        androidx.compose.material3.Button(onClick = { viewModel.setSpeedLimit(speedText.toLongOrNull() ?: 0) }) {
            Text("Apply speed limit")
        }

        Text("UI refresh rate: ${settings.refreshHz} Hz", style = MaterialTheme.typography.titleMedium)
        var refresh by remember(settings.refreshHz) { mutableFloatStateOf(settings.refreshHz.toFloat()) }
        Slider(
            value = refresh,
            onValueChange = { refresh = it },
            valueRange = 10f..30f,
            onValueChangeFinished = { viewModel.setRefreshHz(refresh.toInt()) },
        )

        Text("Theme", style = MaterialTheme.typography.titleMedium)
        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            listOf(AppSettings.THEME_SYSTEM, AppSettings.THEME_LIGHT, AppSettings.THEME_DARK).forEach { mode ->
                androidx.compose.foundation.layout.Column {
                    RadioButton(selected = settings.themeMode == mode, onClick = { viewModel.setThemeMode(mode) })
                    Text(mode.replaceFirstChar { it.uppercase() }, style = MaterialTheme.typography.labelSmall)
                }
            }
        }
    }
}
