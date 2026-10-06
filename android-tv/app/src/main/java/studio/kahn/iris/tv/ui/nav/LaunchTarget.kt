package studio.kahn.iris.tv.ui.nav

import android.app.SearchManager
import android.content.Intent
import androidx.compose.runtime.Immutable

/** Where something outside the app asks it to go: the launcher, the assistant, the remote. */
@Immutable
sealed interface LaunchTarget {
    /** A Watch Next or channel program: `iris://watch/INFOHASH/IDX`. */
    data class Watch(val infohash: String, val fileIdx: Int) : LaunchTarget

    /** Voice search ([query], play the top hit) or the remote's search key (no query). */
    data class Search(val query: String? = null) : LaunchTarget

    /** `iris://home`. */
    data object Home : LaunchTarget

    /** Kept while the TV is not paired yet, to be honoured once it is. The search key is not. */
    val keepsUntilPaired: Boolean
        get() = this is Watch || (this is Search && query != null)

    /** The route this lands on (above Home). */
    fun route(): Any = when (this) {
        is Watch -> Routes.Watch(infohash, fileIdx)
        is Search -> Routes.Search(query, autoPlay = query != null)
        Home -> Routes.Home
    }

    companion object {
        const val ACTION_MEDIA_PLAY_FROM_SEARCH = "android.media.action.MEDIA_PLAY_FROM_SEARCH"

        /** The target of an intent's parts, or null when it is a plain launch. */
        fun of(action: String?, scheme: String?, host: String?, path: List<String>, query: String?): LaunchTarget? =
            when (action) {
                Intent.ACTION_SEARCH, ACTION_MEDIA_PLAY_FROM_SEARCH ->
                    query?.trim()?.takeIf { it.isNotEmpty() }?.let { Search(it) }
                Intent.ACTION_VIEW -> if (scheme != "iris") null else when (host) {
                    "watch" -> {
                        val infohash = path.getOrNull(0)?.takeIf { it.isNotBlank() }
                        val idx = path.getOrNull(1)?.toIntOrNull()
                        if (infohash != null && idx != null && idx >= 0) Watch(infohash, idx) else null
                    }
                    "home" -> Home
                    else -> null
                }
                else -> null
            }
    }
}

fun Intent?.launchTarget(): LaunchTarget? {
    if (this == null) return null
    return LaunchTarget.of(
        action = action,
        scheme = data?.scheme,
        host = data?.host,
        path = data?.pathSegments.orEmpty(),
        query = getStringExtra(SearchManager.QUERY),
    )
}
