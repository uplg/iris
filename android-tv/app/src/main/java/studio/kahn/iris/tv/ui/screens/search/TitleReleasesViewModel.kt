package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.TitleCard
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.format.kindWord

/** What the title's aside shows: the card picked, else what its first release knows. */
@Immutable
data class TitleHead(
    val title: String,
    val meta: String?,
    val posterUrl: String?,
    val inLibrary: Boolean,
)

/** A season filter choice; [season] null = every release. */
@Immutable
data class SeasonOption(val season: Int?, val label: String)

@Immutable
data class TitleReleasesUiState(
    val query: String,
    val tmdbId: Long,
    val card: TitleCard? = null,
    val results: Loadable<SearchPage> = Loadable.Loading,
    val loadingMore: Boolean = false,
    val moreError: UiError? = null,
    val language: String? = null,
    val season: Int? = null,
) {
    val rows: List<SearchResult> get() = results.valueOrNull?.rows.orEmpty()
    private val bySeason: List<SearchResult> get() = if (season == null) rows else rows.filter { it.parsedSeason == season }
    val shown: List<SearchResult> get() = filterLanguage(bySeason, language)
    val audio: List<AudioOption> get() = audioOptions(bySeason, language)
    val seasons: List<SeasonOption> get() = seasonOptions(rows, season)

    val head: TitleHead
        get() {
            val match = rows.firstOrNull()?.titleMatch
            val title = card?.title ?: match?.title ?: query
            val meta = card?.let(::titleMeta)
                ?: match?.let { listOfNotNull(kindWord(it.kind), it.year?.toString()).joinToString(" · ") }
            val owned = card?.collectionId != null || rows.any { ownedFile(it) != null }
            return TitleHead(title, meta?.ifEmpty { null }, card?.posterUrl ?: rows.firstNotNullOfOrNull { it.posterUrl }, owned)
        }

    val summaryLine: String?
        get() = (results as? Loadable.Ready)?.value?.let { summary(0, shown.size, it.providers) }
}

/** "All seasons", then each season the loaded releases carry, when there are several. */
fun seasonOptions(rows: List<SearchResult>, selected: Int?): List<SeasonOption> {
    val seasons = (rows.mapNotNull { it.parsedSeason } + listOfNotNull(selected)).distinct().sorted()
    if (seasons.size < 2 && selected == null) return emptyList()
    return listOf(SeasonOption(null, "All seasons")) + seasons.map { SeasonOption(it, "Season $it") }
}

/**
 * A title's releases (web `/search?title=`): the trackers asked for [query]
 * narrowed to the TMDB title [tmdbId], page after page, then filtered by
 * season and audio among what is loaded.
 */
class TitleReleasesViewModel(
    private val container: AppContainer,
    query: String,
    tmdbId: Long,
    private val kind: SearchKind,
    private val sort: SearchSort,
) : ViewModel() {
    private val mutable = MutableStateFlow(TitleReleasesUiState(query = query, tmdbId = tmdbId, card = SearchMemory.title(tmdbId)))
    val state: StateFlow<TitleReleasesUiState> = mutable.asStateFlow()
    val grabber = Grabber(container, viewModelScope)
    private var searchJob: Job? = null

    init {
        if (mutable.value.card == null) {
            viewModelScope.launch {
                val card = runCatching { container.api().searchTitles(query) }.getOrNull()
                    ?.also(SearchMemory::keepTitles)
                    ?.firstOrNull { it.tmdbId == tmdbId }
                if (card != null) mutable.update { it.copy(card = card) }
            }
        }
        retry()
    }

    fun retry() {
        searchJob?.cancel()
        mutable.update { it.copy(results = Loadable.Loading, loadingMore = false, moreError = null) }
        searchJob = viewModelScope.launch {
            val next = load(Loadable.Loading) { SearchPage.first(fetch(1)) }
            next.valueOrNull?.let { SearchMemory.keep(it.rows) }
            mutable.update { it.copy(results = next) }
        }
    }

    fun loadMore() {
        val s = mutable.value
        val page = (s.results as? Loadable.Ready)?.value ?: return
        val next = page.next ?: return
        if (s.loadingMore || s.moreError != null) return
        mutable.update { it.copy(loadingMore = true) }
        viewModelScope.launch {
            try {
                val res = fetch(next)
                SearchMemory.keep(res.results)
                mutable.update { cur ->
                    val ready = cur.results.valueOrNull ?: return@update cur.copy(loadingMore = false)
                    cur.copy(results = Loadable.Ready(ready.plus(res)), loadingMore = false)
                }
            } catch (e: Exception) {
                mutable.update { it.copy(loadingMore = false, moreError = e.toUiError()) }
            }
        }
    }

    fun retryMore() {
        mutable.update { it.copy(moreError = null) }
        loadMore()
    }

    fun setLanguage(tag: String?) = mutable.update { it.copy(language = tag) }

    fun setSeason(season: Int?) = mutable.update { it.copy(season = season, language = null) }

    fun grab(r: SearchResult) {
        val owned = ownedFile(r)
        if (owned != null) grabber.playOwned(owned) else grabber.run(releaseKey(r), r.grabTarget())
    }

    private suspend fun fetch(page: Int) = container.api().search(
        q = mutable.value.query,
        page = page,
        limit = SEARCH_PAGE_SIZE,
        sortBy = sort.sortBy,
        order = sort.order,
        kind = kind.apiKind,
        tmdbId = mutable.value.tmdbId,
    )
}
