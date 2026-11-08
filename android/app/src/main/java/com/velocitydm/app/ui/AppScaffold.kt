package com.velocitydm.app.ui

// SPDX-License-Identifier: GPL-3.0-or-later

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.lifecycle.viewmodel.compose.viewModel
import com.velocitydm.app.ui.screens.AddScreen
import com.velocitydm.app.ui.screens.DownloadsScreen
import com.velocitydm.app.ui.screens.SettingsScreen

/** Root navigation: three Material 3 tabs (Downloads, Add, Settings). */
@Composable
fun VelocityDmApp(prefillUrl: String?) {
    val viewModel: com.velocitydm.app.ui.DownloadsViewModel = viewModel()
    val tab = rememberSaveable { mutableIntStateOf(0) }
    Scaffold(
        bottomBar = {
            NavigationBar {
                listOf("Downloads" to 0, "Add" to 1, "Settings" to 2).forEach { (label, index) ->
                    NavigationBarItem(
                        selected = tab.intValue == index,
                        onClick = { tab.intValue = index },
                        icon = {},
                        label = { Text(label) },
                    )
                }
            }
        },
    ) { padding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding),
        ) {
            when (tab.intValue) {
                0 -> DownloadsScreen(viewModel)
                1 -> AddScreen(viewModel, prefillUrl)
                else -> SettingsScreen()
            }
        }
    }
}
