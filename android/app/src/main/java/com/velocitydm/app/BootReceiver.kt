package com.velocitydm.app.service

// SPDX-License-Identifier: GPL-3.0-or-later

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.velocitydm.app.data.db.AppDatabase
import com.velocitydm.app.data.db.DownloadEntity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/** After a reboot, re-queues unfinished downloads and restarts the service. */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                val dao = AppDatabase.get(context).downloadDao()
                val unfinished = dao.unfinished()
                for (entity in unfinished) {
                    if (entity.state == DownloadEntity.STATE_RUNNING) {
                        dao.setState(entity.id, DownloadEntity.STATE_QUEUED)
                    }
                }
                if (unfinished.isNotEmpty()) {
                    context.startService(DownloadService.intent(context))
                }
            } finally {
                pending.finish()
            }
        }
    }
}
