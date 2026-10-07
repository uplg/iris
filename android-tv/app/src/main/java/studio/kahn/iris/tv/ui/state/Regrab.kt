package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.MutableStateFlow
import retrofit2.HttpException
import studio.kahn.iris.tv.data.IngestResponse
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.ui.screens.player.deadSwarmError
import studio.kahn.iris.tv.ui.screens.player.retrySearchQuery

/** Where a refused "Download again" lands: Search, [query] typed and run, [notice] saying why. */
@Immutable
data class SearchInstead(val query: String, val notice: String)

/** A reclaimed release, and what to search for when it can't come back ([retrySearchQuery]). */
data class Reclaimed(
    val infohash: String,
    /** The collection's title, when the release has one. */
    val title: String?,
    val season: Long? = null,
    val episode: Long? = null,
    /** The release's own name, the query when there is no title. */
    val name: String? = null,
)

sealed interface Regrabbed {
    data class Back(val res: IngestResponse) : Regrabbed
    data class Refused(val search: SearchInstead) : Regrabbed
}

/**
 * A reclaimed release fetched again, the one way (history, a title's page, the player; web
 * `fetchAgainOrSearch`): `/regrab` re-ingests it from the provenance the server recorded, so
 * the same release comes back and the saved positions apply again. A refusal (the tracker
 * turned off, no seeders, the tracker lost the release, any other 4xx) is [Regrabbed.Refused]:
 * the search to open instead. Anything else (no answer, a server failure) is thrown.
 */
suspend fun regrab(api: IrisApi, r: Reclaimed): Regrabbed = try {
    Regrabbed.Back(api.regrabTorrent(r.infohash))
} catch (e: HttpException) {
    if (!isRefusal(e.code())) throw e
    val q = retrySearchQuery(r.title, r.season, r.episode, r.name)
    Regrabbed.Refused(SearchInstead(q, "${refusalWords(e.toUiError())} Here are other releases of $q."))
}

// 401 and 426 are the session and the version gate, which the app answers itself.
internal fun isRefusal(status: Int): Boolean = status in 400..499 && status != 401 && status != 426

internal fun refusalWords(e: UiError): String = when {
    e.code == "provider_off" -> "This tracker is turned off in Admin."
    e.code == "dead_torrent" || e.code == "stalled" || deadSwarmError.containsMatchIn(e.message) ->
        "Nobody shares this release any more."
    // `provider: …` is the tracker's own words, not for people.
    e.status == 404 || e.message.startsWith("provider:", ignoreCase = true) -> "Its tracker no longer has this release."
    else -> e.message
}

/** A screen model's [SearchInstead] to open, taken once by [SearchInsteadEffect]. */
class SearchInsteadEvent {
    internal val pending = MutableStateFlow<SearchInstead?>(null)

    fun send(search: SearchInstead) {
        pending.value = search
    }
}

/** Opens the search [event] asks for, once. */
@Composable
fun SearchInsteadEffect(event: SearchInsteadEvent, onSearch: (SearchInstead) -> Unit) {
    val search by event.pending.collectAsStateWithLifecycle()
    LaunchedEffect(search) {
        val s = search ?: return@LaunchedEffect
        event.pending.value = null
        onSearch(s)
    }
}
