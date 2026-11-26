package com.velocitydm.android

import android.os.Bundle
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.activity.viewModels
import androidx.activity.result.contract.ActivityResultContracts
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.lifecycleScope
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.floatingactionbutton.FloatingActionButton
import com.velocitydm.android.browser.BrowserActivity
import com.velocitydm.android.data.DownloadsViewModel
import com.velocitydm.android.data.RemotePrefs
import com.velocitydm.android.ui.AddDialog
import com.velocitydm.android.ui.Chrome
import com.velocitydm.android.ui.DownloadsAdapter
import com.velocitydm.android.ui.Md3Theme
import com.velocitydm.android.ui.RemoteDialog
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * Downloads screen — Android View-based Material 3 (S12-e).
 *
 * Programmatic View tree: MaterialToolbar + RecyclerView of download
 * cards + FAB (add URL). Wallpaper dynamic color on Android 12+,
 * seeded Kotlin Md3 port below (see ui/Chrome.kt). VIEW intents
 * (http/https downloads, .torrent, magnet) auto-add, same as the
 * previous Compose UI.
 */
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

    private val browserLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            val data = result.data ?: return@registerForActivityResult
            val url = data.getStringExtra(BrowserActivity.EXTRA_URL) ?: return@registerForActivityResult
            val name = data.getStringExtra(BrowserActivity.EXTRA_NAME)
            viewModel.add(url, name)
        }

    private lateinit var chrome: Chrome
    private lateinit var adapter: DownloadsAdapter
    private lateinit var emptyView: TextView
    private lateinit var staleBanner: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        chrome = Chrome(this)
        Md3Theme.apply(this)
        setContentView(buildViews())

        // handle VIEW intents (http download links, .torrent, magnet)
        intent?.dataString?.let { viewModel.add(it) }

        // S8-W1: apply the persisted engine mode (remote default OFF —
        // local behavior unchanged until the user pairs a daemon)
        viewModel.applyMode(RemotePrefs.read(this))

        lifecycleScope.launch {
            viewModel.tasks.collect { list ->
                adapter.submit(list)
                emptyView.visibility = if (list.isEmpty()) View.VISIBLE else View.GONE
            }
        }
        lifecycleScope.launch {
            viewModel.remoteStatus.collect { st ->
                staleBanner.visibility =
                    if (st.active && st.stale) View.VISIBLE else View.GONE
                staleBanner.text = "${st.failure ?: "daemon unreachable"} — rows stale"
            }
        }
    }

    private fun buildViews(): View {
        val root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

        val toolbar = MaterialToolbar(this).apply {
            title = "VelocityDM"
            setTitleTextColor(chrome.primary())
            inflateMenu(R.menu.main)
            setOnMenuItemClickListener { item: MenuItem ->
                when (item.itemId) {
                    R.id.action_add -> {
                        showAddDialog(); true
                    }
                    R.id.action_remote -> {
                        showRemoteDialog(); true
                    }
                    R.id.action_browser -> {
                        browserLauncher.launch(
                            android.content.Intent(this@MainActivity, BrowserActivity::class.java)
                        )
                        true
                    }
                    else -> false
                }
            }
        }
        root.addView(
            toolbar,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )

        // S8-W1: dead-daemon banner for remote mode (rows freeze while
        // the 2 s probe retries; recovery is automatic)
        staleBanner = TextView(this).apply {
            textSize = 12f
            setTextColor(chrome.onErrorContainer())
            setBackgroundColor(chrome.errorContainer())
            setPadding(dp(12), dp(8), dp(12), dp(8))
            visibility = View.GONE
        }
        root.addView(
            staleBanner,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        )

        val content = FrameLayout(this)
        root.addView(
            content,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)
        )

        adapter = DownloadsAdapter(chrome).apply {
            onPause = { viewModel.pause(it) }
            onResume = { viewModel.resume(it) }
            onRemove = { viewModel.remove(it) }
        }
        val list = RecyclerView(this).apply {
            layoutManager = LinearLayoutManager(this@MainActivity)
            setAdapter(adapter)
            val pad = dp(8)
            setPadding(pad, pad, pad, pad)
            clipToPadding = false
        }
        content.addView(
            list,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT
            )
        )

        emptyView = TextView(this).apply {
            text = "No downloads yet.\nUse Add, the built-in browser, or share\na link from another app."
            textSize = 14f
            setTextColor(chrome.onSurfaceVariant())
            gravity = android.view.Gravity.CENTER
            visibility = View.GONE
        }
        content.addView(
            emptyView,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT
            )
        )

        val fab = FloatingActionButton(this).apply {
            setImageResource(android.R.drawable.ic_input_add)
            backgroundTintList = android.content.res.ColorStateList.valueOf(chrome.primaryContainer())
            imageTintList = android.content.res.ColorStateList.valueOf(chrome.onPrimaryContainer())
            setOnClickListener { showAddDialog() }
        }
        content.addView(
            fab,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT,
                android.view.Gravity.BOTTOM or android.view.Gravity.END
            ).apply { marginEnd = dp(16); bottomMargin = dp(16) }
        )

        return root
    }

    private fun showAddDialog() {
        AddDialog(this, chrome) { url, name, limitBps ->
            viewModel.add(url, name.ifBlank { null }, limitBps)
        }.show()
    }

    private fun showRemoteDialog() {
        RemoteDialog(this, chrome) { settings ->
            RemotePrefs.write(this, settings)
            viewModel.applyMode(settings)
        }.show()
    }

    private fun dp(v: Int): Int = (v * resources.displayMetrics.density).toInt()
}
