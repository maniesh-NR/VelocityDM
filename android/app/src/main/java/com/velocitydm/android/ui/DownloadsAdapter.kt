package com.velocitydm.android.ui

import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.R as appcompatR
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.button.MaterialButton
import com.google.android.material.card.MaterialCardView
import com.google.android.material.progressindicator.LinearProgressIndicator
import com.velocitydm.shared.engine.TaskState
import com.velocitydm.shared.format.fmtBytes
import com.velocitydm.shared.format.fmtSpeed

/**
 * Download list — one Material 3 card per task (View-based, S12-e),
 * full parity with the previous Compose rows: filename + semantic state
 * label, LinearProgressIndicator, done/total/speed line, pause/resume
 * and remove actions, and the per-task error message.
 */
class DownloadsAdapter(private val chrome: Chrome) : RecyclerView.Adapter<DownloadsAdapter.TaskViewHolder>() {

    private val items = mutableListOf<TaskState>()

    var onPause: (Long) -> Unit = {}
    var onResume: (Long) -> Unit = {}
    var onRemove: (Long) -> Unit = {}

    fun submit(list: List<TaskState>) {
        items.clear()
        items.addAll(list)
        notifyDataSetChanged()
    }

    override fun getItemCount(): Int = items.size

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): TaskViewHolder {
        val ctx = parent.context
        val card = MaterialCardView(ctx).apply {
            setCardBackgroundColor(android.content.res.ColorStateList.valueOf(chrome.surfaceContainer()))
            setRadius(dp(ctx, 12).toFloat())
            cardElevation = 0f
            setContentPadding(dp(ctx, 12), dp(ctx, 12), dp(ctx, 12), dp(ctx, 12))
            useCompatPadding = false
        }
        val col = LinearLayout(ctx).apply { orientation = LinearLayout.VERTICAL }

        // Row 1: filename + state label
        val row1 = LinearLayout(ctx).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
        val filename = TextView(ctx).apply {
            textSize = 16f
            setTextColor(chrome.onSurface())
            maxLines = 1
            ellipsize = android.text.TextUtils.TruncateAt.END
        }
        row1.addView(
            filename,
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
        )
        val state = TextView(ctx).apply { textSize = 11f }
        row1.addView(
            state,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )
        col.addView(row1, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        // Progress
        val progress = LinearProgressIndicator(ctx).apply {
            max = 100
            setIndicatorColor(chrome.primary())
            trackCornerRadius = dp(ctx, 3)
        }
        col.addView(
            progress,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(ctx, 8)).apply { topMargin = dp(ctx, 6); bottomMargin = dp(ctx, 6) }
        )

        // Row 3: stats + actions
        val row3 = LinearLayout(ctx).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
        val stats = TextView(ctx).apply {
            textSize = 11f
            setTextColor(chrome.onSurfaceVariant())
        }
        row3.addView(stats, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        val action = borderlessButton(ctx, "")
        row3.addView(action)
        val remove = borderlessButton(ctx, "Remove")
        row3.addView(remove)
        col.addView(row3, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        // Error line
        val error = TextView(ctx).apply {
            textSize = 11f
            setTextColor(chrome.error())
            visibility = View.GONE
        }
        col.addView(error, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        // Notice line (S8-W1: the daemon's advisory hints — proxy
        // warnings, recovery notes; local tasks never set it)
        val notice = TextView(ctx).apply {
            textSize = 11f
            setTextColor(chrome.onSurfaceVariant())
            visibility = View.GONE
        }
        col.addView(notice, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        card.addView(
            col,
            ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )
        val lp = RecyclerView.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
        ).apply { bottomMargin = dp(ctx, 6) }
        card.layoutParams = lp

        return TaskViewHolder(card, filename, state, progress, stats, action, remove, error, notice)
    }

    private fun borderlessButton(ctx: android.content.Context, text: String): MaterialButton =
        MaterialButton(ctx, null, appcompatR.attr.borderlessButtonStyle).apply {
            this.text = text
            setTextColor(chrome.primary())
        }

    override fun onBindViewHolder(holder: TaskViewHolder, position: Int) {
        val t = items[position]
        holder.filename.text = t.filename
        holder.state.text = t.state
        holder.state.setTextColor(
            when (t.state) {
                "completed" -> chrome.tertiary()
                "failed" -> chrome.error()
                "downloading", "verifying" -> chrome.primary()
                "seeding" -> chrome.tertiary()
                else -> chrome.onSurfaceVariant()
            }
        )
        val frac = if (t.size > 0) (t.done.toFloat() / t.size).coerceIn(0f, 1f) else 0f
        holder.progress.setProgress((frac * 100).toInt())
        holder.stats.text = "${fmtBytes(t.done)} / ${if (t.size > 0) fmtBytes(t.size) else "?"}  ${fmtSpeed(t.speed)}"
        when (t.state) {
            "downloading", "probing", "queued" -> {
                holder.action.visibility = View.VISIBLE
                holder.action.text = "Pause"
                holder.action.setOnClickListener { onPause(t.id) }
            }
            "paused", "failed" -> {
                // S8-W1: a daemon-paused task (stop window, userPaused
                // = false) would instantly re-stop on resume — no
                // action is offered; the notice line explains why.
                if (t.state == "failed" || t.userPaused != false) {
                    holder.action.visibility = View.VISIBLE
                    holder.action.text = "Resume"
                    holder.action.setOnClickListener { onResume(t.id) }
                } else {
                    holder.action.visibility = View.GONE
                }
            }
            // verifying/seeding/completed: no pause (the card keeps
            // Remove in every state)
            else -> holder.action.visibility = View.GONE
        }
        holder.remove.setOnClickListener { onRemove(t.id) }
        if (t.error != null) {
            holder.error.text = t.error
            holder.error.visibility = View.VISIBLE
        } else {
            holder.error.visibility = View.GONE
        }
        if (t.notice != null) {
            holder.notice.text = t.notice
            holder.notice.visibility = View.VISIBLE
        } else {
            holder.notice.visibility = View.GONE
        }
    }

    class TaskViewHolder(
        view: View,
        val filename: TextView,
        val state: TextView,
        val progress: LinearProgressIndicator,
        val stats: TextView,
        val action: MaterialButton,
        val remove: MaterialButton,
        val error: TextView,
        val notice: TextView,
    ) : RecyclerView.ViewHolder(view)

    private fun dp(ctx: android.content.Context, v: Int): Int =
        (v * ctx.resources.displayMetrics.density).toInt()
}
