package com.velocitydm.android.ui

import android.app.Activity
import android.text.InputType
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.Switch
import android.widget.TextView
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.textfield.TextInputEditText
import com.google.android.material.textfield.TextInputLayout
import com.velocitydm.android.data.RemotePrefs

/**
 * Remote-daemon settings dialog (S8-W1): enable switch, host, port,
 * bearer token (`velocitydm lan-token` prints it on the desktop) and
 * the optional manually-entered certificate fingerprint that pre-pins
 * the daemon (closing the TOFU first-connect window). A Test button
 * probes `daemon.ping` and reports honestly. Saved values apply
 * immediately via the ViewModel's mode switch.
 */
class RemoteDialog(
    private val activity: Activity,
    @Suppress("UNUSED_PARAMETER") chrome: Chrome,
    private val onApply: (RemotePrefs.Settings) -> Unit,
) {
    fun show() {
        val ctx = activity
        val current = RemotePrefs.read(ctx)

        val col = LinearLayout(ctx).apply {
            orientation = LinearLayout.VERTICAL
            val pad = dp(24)
            setPadding(pad, dp(8), pad, 0)
        }

        val enable = Switch(ctx).apply {
            text = "Control a desktop daemon over LAN"
            isChecked = current.enabled
        }
        col.addView(
            enable,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
                .apply { bottomMargin = dp(8) }
        )

        fun field(hint: String, prefill: String, password: Boolean = false): TextInputEditText {
            val layout = TextInputLayout(
                ctx, null, com.google.android.material.R.attr.textInputStyle
            ).apply { this.hint = hint }
            val edit = TextInputEditText(ctx).apply {
                maxLines = 1
                setText(prefill)
                if (password) {
                    inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
                }
            }
            layout.addView(
                edit,
                ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            )
            col.addView(
                layout,
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
                    .apply { bottomMargin = dp(8) }
            )
            return edit
        }

        val host = field("Desktop host (IP or name, no https://)", current.host)
        val port = field("Port (default 36990)", current.port.toString())
        val token = field("Bearer token (velocitydm lan-token)", current.token, password = true)
        val fingerprint = field(
            "Certificate fingerprint (optional — from `velocitydm lan-token`)",
            current.fingerprint
        )

        val status = TextView(ctx).apply {
            textSize = 12f
            setPadding(0, dp(4), 0, dp(4))
        }
        col.addView(
            status,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )

        fun collect() = RemotePrefs.Settings(
            enabled = enable.isChecked,
            host = host.text?.toString()?.trim() ?: "",
            port = port.text?.toString()?.trim()?.toIntOrNull() ?: 36990,
            token = token.text?.toString()?.trim() ?: "",
            fingerprint = fingerprint.text?.toString()?.trim() ?: "",
        )

        val dialog = MaterialAlertDialogBuilder(ctx)
            .setTitle("Remote daemon")
            .setView(col)
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Save", null)
            .setNeutralButton("Test", null)
            .create()
        dialog.show()

        dialog.getButton(androidx.appcompat.app.AlertDialog.BUTTON_NEUTRAL)?.setOnClickListener {
            val s = collect()
            val cfg = s.toConfig()
            if (cfg == null) {
                status.text = s.invalidReason ?: "invalid settings"
            } else {
                status.text = "connecting…"
                Thread {
                    val tofu = com.velocitydm.android.data.RemotePrefs.tofuStore(ctx)
                    val rpc = com.velocitydm.shared.remote.RemoteRpc(cfg, com.velocitydm.shared.remote.TofuTrustManager(tofu))
                    val res = rpc.ping()
                    val msg = res.fold(
                        onSuccess = { "connected: $it" },
                        onFailure = { it.message },
                    )
                    activity.runOnUiThread { status.text = msg }
                }.start()
            }
        }

        dialog.getButton(androidx.appcompat.app.AlertDialog.BUTTON_POSITIVE)?.setOnClickListener {
            onApply(collect())
            dialog.dismiss()
        }
    }

    private fun dp(v: Int): Int = (v * activity.resources.displayMetrics.density).toInt()
}
