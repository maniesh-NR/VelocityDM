package com.velocitydm.android.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import com.velocitydm.android.MainActivity
import com.velocitydm.android.R
import com.velocitydm.android.engine.TaskState

/**
 * Foreground download service: keeps the engine alive and posts progress
 * notifications while anything is active (Android 8+ requirement).
 */
class DownloadService : Service() {

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(NOTIF_ID, buildNotification(this, null))
        // stop ourselves when nothing is running (managed from the ViewModel)
        return START_STICKY
    }

    companion object {
        private const val CHANNEL = "downloads"
        private const val NOTIF_ID = 41

        fun ensureRunning(context: Context) {
            val intent = Intent(context, DownloadService::class.java)
            if (Build.VERSION.SDK_INT >= 26) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        fun notify(context: Context, t: TaskState?) {
            val nm = context.getSystemService(NotificationManager::class.java) ?: return
            if (nm.getNotificationChannel(CHANNEL) == null) {
                nm.createNotificationChannel(
                    NotificationChannel(
                        CHANNEL,
                        context.getString(R.string.notif_channel),
                        NotificationManager.IMPORTANCE_LOW,
                    ).apply {
                        description = context.getString(R.string.notif_channel_desc)
                    }
                )
            }
            if (t == null) return
            val text = when (t.state) {
                "downloading" -> "%d%%  %s".format(
                    if (t.size > 0) (100 * t.done / t.size).toInt() else 0,
                    humanSpeed(t.speed),
                )
                else -> t.state
            }
            val n: Notification = NotificationCompat.Builder(context, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_sys_download)
                .setContentTitle(t.filename)
                .setContentText(text)
                .setOngoing(t.state == "downloading")
                .setOnlyAlertOnce(true)
                .setProgress(
                    100,
                    if (t.size > 0) ((100 * t.done / t.size).toInt()) else 0,
                    t.size == 0L && t.state == "downloading",
                )
                .setContentIntent(
                    android.app.PendingIntent.getActivity(
                        context, 0,
                        Intent(context, MainActivity::class.java),
                        android.app.PendingIntent.FLAG_IMMUTABLE,
                    )
                )
                .build()
            nm.notify(t.id.toInt() + 1000, n)
        }

        private fun humanSpeed(bps: Long): String {
            if (bps <= 0) return "-"
            if (bps < 1024) return "$bps B/s"
            if (bps < 1024 * 1024) return "%.0f KiB/s".format(bps / 1024.0)
            return "%.1f MiB/s".format(bps / 1024.0 / 1024)
        }
    }
}
