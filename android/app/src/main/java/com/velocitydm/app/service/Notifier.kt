package com.velocitydm.app.service

// SPDX-License-Identifier: GPL-3.0-or-later

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import com.velocitydm.app.MainActivity
import com.velocitydm.app.R

/** Notification plumbing: summary, per-download progress, terminal states. */
class Notifier(private val context: Context) {

    private val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    init {
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_DOWNLOADS,
                context.getString(R.string.channel_downloads),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_STATUS,
                context.getString(R.string.channel_status),
                NotificationManager.IMPORTANCE_DEFAULT,
            ),
        )
    }

    fun buildSummary(text: String): Notification =
        NotificationCompat.Builder(context, CHANNEL_DOWNLOADS)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(text)
            .setOngoing(true)
            .build()

    fun progress(id: Long, filename: String) {
        val notification = NotificationCompat.Builder(context, CHANNEL_DOWNLOADS)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle(filename)
            .setContentText(context.getString(R.string.downloading))
            .setOngoing(true)
            .setProgress(0, 0, true)
            .addAction(0, context.getString(R.string.pause), action(DownloadService.ACTION_PAUSE, id, 1000 + id.toInt()))
            .addAction(0, context.getString(R.string.cancel), action(DownloadService.ACTION_CANCEL, id, 2000 + id.toInt()))
            .build()
        manager.notify(id.toInt(), notification)
    }

    fun updateText(id: Long, text: String) {
        val notification = NotificationCompat.Builder(context, CHANNEL_DOWNLOADS)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle(text)
            .setOngoing(true)
            .build()
        manager.notify(id.toInt(), notification)
    }

    fun completed(id: Long, filename: String) {
        manager.cancel(id.toInt())
        manager.notify(
            (id % 100000).toInt() + 500000,
            NotificationCompat.Builder(context, CHANNEL_STATUS)
                .setSmallIcon(R.drawable.ic_launcher_foreground)
                .setContentTitle(context.getString(R.string.download_completed))
                .setContentText(filename)
                .setContentIntent(contentIntent())
                .setAutoCancel(true)
                .build(),
        )
    }

    fun failed(id: Long, filename: String, error: String) {
        manager.cancel(id.toInt())
        manager.notify(
            (id % 100000).toInt() + 600000,
            NotificationCompat.Builder(context, CHANNEL_STATUS)
                .setSmallIcon(R.drawable.ic_launcher_foreground)
                .setContentTitle(context.getString(R.string.download_failed))
                .setContentText("$filename: $error")
                .setContentIntent(contentIntent())
                .setAutoCancel(true)
                .build(),
        )
    }

    private fun action(action: String, id: Long, requestCode: Int): PendingIntent =
        PendingIntent.getService(
            context,
            requestCode,
            DownloadService.actionIntent(context, action, id),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

    private fun contentIntent(): PendingIntent =
        PendingIntent.getActivity(
            context,
            0,
            Intent(context, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

    companion object {
        const val CHANNEL_DOWNLOADS = "downloads"
        const val CHANNEL_STATUS = "status"
    }
}
