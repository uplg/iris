package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.runtime.Immutable
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.SearchResponse
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.toUiError

/** The tracker pages read for one search, and how the next one comes. */
@Immutable
data class ReleasePages(
    /** Null before any search. */
    val results: Loadable<SearchPage>? = null,
    val loadingMore: Boolean = false,
    val moreError: UiError? = null,
    /**
     * The last page added nothing to what the filters show: the next page waits for the
     * viewer ("Show more releases"), so a filter matching little never pages every tracker
     * unattended (each page asks them all).
     */
    val waitsForViewer: Boolean = false,
) {
    val hasNext: Boolean get() = results?.valueOrNull?.next != null

    /** The end of the list in sight loads the next page by itself. */
    val autoLoads: Boolean get() = hasNext && !loadingMore && moreError == null && !waitsForViewer
}

/** A page read by itself goes on by itself only when it showed something new. */
fun keepsLoadingByItself(shownBefore: Int, shownAfter: Int): Boolean = shownAfter > shownBefore

/**
 * The pages of a search, for the search screen and a title's releases: the first page, the
 * next ones (by itself at the end of the list, or asked), a retry, the stale ones dropped when
 * a new search starts. [shown] counts the releases the screen's filters keep of a page.
 */
class ReleasePager(
    private val scope: CoroutineScope,
    private val shown: (SearchPage) -> Int,
    private val fetch: suspend (page: Int) -> SearchResponse,
) {
    private val mutable = MutableStateFlow(ReleasePages())
    val state: StateFlow<ReleasePages> = mutable.asStateFlow()
    private var firstJob: Job? = null
    private var moreJob: Job? = null

    /** A new search: page 1, then [onFirst] with it. */
    fun first(onFirst: (SearchPage) -> Unit = {}) {
        firstJob?.cancel()
        moreJob?.cancel()
        mutable.value = ReleasePages(results = Loadable.Loading)
        firstJob = scope.launch {
            val next = load(Loadable.Loading) { SearchPage.first(fetch(1)) }
            next.valueOrNull?.let { SearchMemory.keep(it.rows) }
            mutable.update { it.copy(results = next) }
            next.valueOrNull?.let(onFirst)
        }
    }

    /** The next page: [byViewer] when asked ("Show more", a retry), else the end of the list came in sight. */
    fun more(byViewer: Boolean = false) {
        val s = mutable.value
        val page = (s.results as? Loadable.Ready)?.value ?: return
        val next = page.next ?: return
        if (s.loadingMore || s.moreError != null) return
        if (!byViewer && s.waitsForViewer) return
        mutable.update { it.copy(loadingMore = true) }
        moreJob = scope.launch {
            try {
                val res = fetch(next)
                SearchMemory.keep(res.results)
                mutable.update { cur ->
                    val ready = (cur.results as? Loadable.Ready)?.value ?: return@update cur.copy(loadingMore = false)
                    val merged = ready.plus(res)
                    cur.copy(
                        results = Loadable.Ready(merged),
                        loadingMore = false,
                        waitsForViewer = !keepsLoadingByItself(shown(ready), shown(merged)),
                    )
                }
            } catch (e: Exception) {
                mutable.update { it.copy(loadingMore = false, moreError = e.toUiError()) }
            }
        }
    }

    fun retryMore() {
        mutable.update { it.copy(moreError = null) }
        more(byViewer = true)
    }

    /** The filters changed: what they show is new, the end of the list may load by itself again. */
    fun filtersChanged() {
        mutable.update { it.copy(waitsForViewer = false) }
    }
}

/** What hold OK does with a release. */
sealed interface HoldAction {
    data class Play(val owned: OwnedFile) : HoldAction
    data class Refuse(val message: String) : HoldAction
    data object Grab : HoldAction
}

/** Hold OK on a release: the copy on disk plays; a confirmed empty swarm is never grabbed (as the web and the release page). */
fun holdAction(r: SearchResult): HoldAction {
    ownedFile(r)?.let { return HoldAction.Play(it) }
    if (isDead(r.seeders)) return HoldAction.Refuse(DEAD_GRAB)
    return HoldAction.Grab
}

const val DEAD_GRAB = "$DEAD: this release cannot be downloaded. Try another one."

/** [holdAction], carried out. */
fun Grabber.grabOrPlay(r: SearchResult) {
    when (val action = holdAction(r)) {
        is HoldAction.Play -> playOwned(action.owned)
        is HoldAction.Refuse -> refuse(releaseKey(r), action.message)
        HoldAction.Grab -> run(releaseKey(r), r.grabTarget())
    }
}
