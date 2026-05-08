package studio.kahn.iris.tv

import android.app.SearchManager
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.lifecycle.lifecycleScope
import androidx.tv.material3.ExperimentalTvMaterial3Api
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.ui.IrisRoot
import studio.kahn.iris.tv.ui.theme.IrisTvTheme

class MainActivity : ComponentActivity() {

    @OptIn(ExperimentalTvMaterial3Api::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val container = (application as IrisApp).container
        // Voice search hand-off from Google Assistant: the system delivers a
        // MEDIA_PLAY_FROM_SEARCH intent with the spoken phrase as the
        // SearchManager.QUERY extra. We propagate it to the SearchScreen
        // start route, which auto-runs the search and auto-plays the top hit.
        val pendingVoiceQuery = intent.voiceQuery()
        // Channel deep-link: a click on a PreviewProgram in the TV home
        // launcher fires `iris://watch/INFOHASH/IDX`. Skip Home, go straight
        // to playback.
        val pendingWatch = intent.watchDeepLink()
        // Refresh the TV channel rows in the background so the launcher
        // shows up-to-date posters/titles next time.
        lifecycleScope.launch {
            runCatching { container.channels.sync(container) }
        }
        setContent {
            IrisTvTheme {
                val session by container.sessionStore.session.collectAsState(initial = null)
                IrisRoot(
                    container = container,
                    isAuthenticated = session != null,
                    pendingVoiceQuery = pendingVoiceQuery,
                    pendingWatch = pendingWatch,
                )
            }
        }
    }
}

private fun Intent?.voiceQuery(): String? {
    if (this == null) return null
    return when (action) {
        Intent.ACTION_SEARCH,
        "android.media.action.MEDIA_PLAY_FROM_SEARCH" ->
            getStringExtra(SearchManager.QUERY)?.takeIf { it.isNotBlank() }
        else -> null
    }
}

/** Parse `iris://watch/INFOHASH/IDX` from a deep-link Intent. */
private fun Intent?.watchDeepLink(): Pair<String, Int>? {
    if (this == null || action != Intent.ACTION_VIEW) return null
    val uri: Uri = data ?: return null
    if (uri.scheme != "iris" || uri.host != "watch") return null
    val segments = uri.pathSegments
    if (segments.size < 2) return null
    val infohash = segments[0].takeIf { it.isNotBlank() } ?: return null
    val idx = segments[1].toIntOrNull() ?: return null
    return infohash to idx
}
