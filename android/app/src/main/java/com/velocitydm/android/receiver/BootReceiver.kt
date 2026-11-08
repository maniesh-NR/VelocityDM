package com.velocitydm.android.receiver

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.velocitydm.android.service.DownloadService

/**
 * After a reboot, restart the foreground service so paused downloads can be
 * resumed from their `.vdmctl` state (IDM-style persistence).
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED) {
            DownloadService.ensureRunning(context)
        }
    }
}
