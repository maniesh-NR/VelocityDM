package com.velocitydm.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.runtime.getValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import com.velocitydm.android.data.DownloadsViewModel
import com.velocitydm.android.ui.DownloadsScreen
import com.velocitydm.android.ui.VelocityTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

class MainActivity : ComponentActivity() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val viewModel: DownloadsViewModel by viewModels {
        object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T {
                return DownloadsViewModel(application, scope) as T
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // handle VIEW intents (http download links, .torrent, magnet)
        val incomingUrl = intent?.dataString
        setContent {
            VelocityTheme {
                DownloadsScreen(viewModel, incomingUrl)
            }
        }
    }
}
