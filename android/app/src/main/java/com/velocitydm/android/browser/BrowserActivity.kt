package com.velocitydm.android.browser

import android.annotation.SuppressLint
import android.content.Intent
import android.os.Bundle
import android.view.MenuItem
import android.view.View
import android.view.ViewGroup
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.button.MaterialButton
import com.google.android.material.textfield.TextInputLayout
import com.velocitydm.android.R
import com.velocitydm.android.ui.Chrome
import com.velocitydm.android.ui.Md3Theme

/**
 * Built-in sniffer browser (1DM-style): watches page traffic for media
 * (m3u8/mpd/video files) and lists everything found for one-tap
 * download. S12-e makes this a REAL Activity (the manifest has declared
 * `.browser.BrowserActivity` since v1.0.0 — the Compose era only had a
 * composable, so the entry pointed at nothing). Returns the chosen
 * media via the activity result.
 */
class BrowserActivity : AppCompatActivity() {

    private lateinit var chrome: Chrome
    private lateinit var address: EditText
    private lateinit var countView: TextView
    private lateinit var sniffList: LinearLayout
    private lateinit var webView: WebView
    private val sniffed = mutableListOf<SniffedMedia>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        chrome = Chrome(this)
        Md3Theme.apply(this)
        setContentView(buildViews())
        loadUrl("https://example.com")
    }

    private fun buildViews(): View {
        val root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

        val toolbar = MaterialToolbar(this).apply {
            setTitleTextColor(chrome.primary())
            title = "Browser"
            inflateMenu(R.menu.browser)
            setOnMenuItemClickListener { item: MenuItem ->
                when (item.itemId) {
                    R.id.action_clear -> {
                        sniffed.clear()
                        renderSniffed()
                        true
                    }
                    else -> false // close handled by navigation
                }
            }
            setNavigationOnClickListener { finish() }
            navigationIcon = androidx.appcompat.content.res.AppCompatResources.getDrawable(
                this@BrowserActivity, android.R.drawable.ic_menu_close_clear_cancel
            )
        }
        root.addView(
            toolbar,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )

        // Address row
        val addressRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = android.view.Gravity.CENTER_VERTICAL
            val pad = dp(12)
            setPadding(pad, dp(4), pad, dp(4))
        }
        val addressLayout = TextInputLayout(
            this, null, com.google.android.material.R.attr.textInputStyle
        ).apply { hint = "Address" }
        address = EditText(this).apply { maxLines = 1 }
        addressLayout.addView(
            address,
            ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )
        addressRow.addView(
            addressLayout,
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
        )
        val go = MaterialButton(this).apply {
            text = "Go"
            setOnClickListener { submitAddress() }
        }
        addressRow.addView(go)
        root.addView(
            addressRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )

        // Sniffed-media list header + items (fixed area, WebView below —
        // same composition as the Compose screen, which showed up to six).
        countView = TextView(this).apply {
            textSize = 12f
            setTextColor(chrome.primary())
            visibility = View.GONE
            val pad = dp(12)
            setPadding(pad, dp(4), pad, dp(4))
        }
        root.addView(
            countView,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )

        val scroll = ScrollView(this)
        sniffList = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        scroll.addView(
            sniffList,
            ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )
        root.addView(
            scroll,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 0.35f)
        )

        // WebView
        webView = WebView(this).apply {
            @SuppressLint("SetJavaScriptEnabled")
            settings.javaScriptEnabled = true
            settings.mediaPlaybackRequiresUserGesture = false
            webViewClient = SniffWebViewClient { media ->
                sniffed.removeAll { it.url == media.url }
                sniffed.add(0, media)
                if (sniffed.size > 20) sniffed.removeLast()
                renderSniffed()
            }
            webChromeClient = WebChromeClient()
        }
        root.addView(
            webView,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 0.65f)
        )

        return root
    }

    private fun submitAddress() {
        val url = address.text?.toString()?.trim() ?: return
        if (com.velocitydm.shared.net.isHttpUrl(url)) loadUrl(url)
    }

    private fun loadUrl(url: String) {
        address.setText(url)
        webView.loadUrl(url)
    }

    private fun renderSniffed() {
        sniffList.removeAllViews()
        if (sniffed.isEmpty()) {
            countView.visibility = View.GONE
            return
        }
        countView.visibility = View.VISIBLE
        countView.text = "${sniffed.size} media found"
        sniffed.take(6).forEach { m ->
            val row = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = android.view.Gravity.CENTER_VERTICAL
                setPadding(dp(10), dp(10), dp(10), dp(10))
            }
            val textCol = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
            val title = TextView(this).apply {
                text = m.title
                textSize = 14f
                setTextColor(chrome.onSurface())
                maxLines = 1
                ellipsize = android.text.TextUtils.TruncateAt.END
            }
            textCol.addView(title)
            val urlView = TextView(this).apply {
                text = m.url
                textSize = 10f
                setTextColor(chrome.onSurfaceVariant())
                maxLines = 1
                ellipsize = android.text.TextUtils.TruncateAt.END
            }
            textCol.addView(urlView)
            row.addView(textCol, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            val get = MaterialButton(this).apply {
                text = "Get"
                setOnClickListener {
                    setResult(
                        RESULT_OK,
                        Intent().putExtra(EXTRA_URL, m.url).putExtra(EXTRA_NAME, m.title)
                    )
                    finish()
                }
            }
            row.addView(get)
            sniffList.addView(
                row,
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
                    .apply { bottomMargin = dp(4) }
            )
        }
    }

    private fun dp(v: Int): Int = (v * resources.displayMetrics.density).toInt()

    companion object {
        const val EXTRA_URL = "url"
        const val EXTRA_NAME = "name"
    }
}

data class SniffedMedia(val url: String, val title: String, val kind: String)

private class SniffWebViewClient(
    private val onSniff: (SniffedMedia) -> Unit,
) : WebViewClient() {
    private val seen = mutableSetOf<String>()

    override fun shouldInterceptRequest(
        view: WebView,
        request: WebResourceRequest,
    ): WebResourceResponse? {
        val url = request.url.toString()
        classify(url)?.let { kind ->
            if (seen.add(url)) {
                val title = url.substringAfterLast('/').substringBefore('?').ifBlank { "media" }
                onSniff(SniffedMedia(url, title, kind))
            }
        }
        return super.shouldInterceptRequest(view, request)
    }

    private fun classify(url: String): String? {
        val u = url.lowercase()
        return when {
            u.contains(".m3u8") -> "hls"
            u.contains(".mpd") -> "dash"
            Regex("\\.(mp4|webm|mkv|mov|m4v)(\\?|$)").containsMatchIn(u) -> "video"
            Regex("\\.(mp3|m4a|flac|opus)(\\?|$)").containsMatchIn(u) -> "audio"
            else -> null
        }
    }
}
