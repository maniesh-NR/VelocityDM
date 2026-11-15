package com.velocitydm.android.ui

import android.app.Activity
import android.text.Editable
import android.text.TextWatcher
import android.view.ViewGroup
import android.widget.LinearLayout
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.textfield.TextInputEditText
import com.google.android.material.textfield.TextInputLayout
import com.velocitydm.shared.format.parseLimit
import com.velocitydm.shared.net.isHttpUrl

/**
 * "New download" dialog (S12-e, View-based) — full parity with the old
 * Compose AddDialog: URL (validated as a real http/https URL, not the
 * old `startsWith("http")`), optional filename override, optional
 * speed limit.
 */
class AddDialog(
    private val activity: Activity,
    @Suppress("UNUSED_PARAMETER") chrome: Chrome,
    private val onAdd: (url: String, filename: String, limitBps: Long) -> Unit,
) {
    fun show() {
        val ctx = activity
        val col = LinearLayout(ctx).apply {
            orientation = LinearLayout.VERTICAL
            val pad = dp(24)
            setPadding(pad, dp(8), pad, 0)
        }

        fun field(hint: String): TextInputEditText {
            val layout = TextInputLayout(
                ctx, null, com.google.android.material.R.attr.textInputStyle
            ).apply { this.hint = hint }
            val edit = TextInputEditText(ctx).apply { maxLines = 1 }
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

        val url = field("URL (http/https)")
        val name = field("File name (optional)")
        val limit = field("Speed limit, e.g. 2MB (optional)")

        val dialog = MaterialAlertDialogBuilder(ctx)
            .setTitle("New download")
            .setView(col)
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Add", null)
            .create()

        url.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
            override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
            override fun afterTextChanged(s: Editable?) {
                dialog.getButton(androidx.appcompat.app.AlertDialog.BUTTON_POSITIVE)
                    ?.isEnabled = isHttpUrl(s?.toString() ?: "")
            }
        })

        dialog.show()
        dialog.getButton(androidx.appcompat.app.AlertDialog.BUTTON_POSITIVE)?.apply {
            isEnabled = false
            setOnClickListener {
                onAdd(url.text?.toString()?.trim() ?: "", name.text?.toString()?.trim() ?: "", parseLimit(limit.text?.toString() ?: ""))
                dialog.dismiss()
            }
        }
    }

    private fun dp(v: Int): Int = (v * activity.resources.displayMetrics.density).toInt()
}
