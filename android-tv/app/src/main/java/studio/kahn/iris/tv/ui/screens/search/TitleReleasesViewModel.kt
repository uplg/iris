package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.api
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.data.LibraryMatch
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.TitleCard
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
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
    val pages: ReleasePages = ReleasePages(results = Loadable.Loading),
    val language: String? = null,
    val season: Int? = null,
) {
    val results: Loadable<SearchPage> get() = pages.results ?: Loadable.Loading
    val loadingMore: Boolean get() = pages.loadingMore
    val moreError: UiError? get() = pages.moreError

    val rows: List<SearchResult> by lazy { results.valueOrNull?.rows.orEmpty() }
    private val bySeason: List<SearchResult> by lazy { bySeason(rows, season) }
    val shown: List<SearchResult> by lazy { filterLanguage(bySeason, language) }
    val audio: List<AudioOption> by lazy { audioOptions(bySeason, language) }
    val seasons: List<SeasonOption> by lazy { seasonOptions(rows, season) }

    /** What the library holds of this title (web: the search's matches narrowed to the title). */
    val matches: List<LibraryMatch> by lazy { results.valueOrNull?.matches.orEmpty().filter { it.tmdbId == tmdbId } }

    val head: TitleHead by lazy {
        val match = rows.firstOrNull()?.titleMatch
        val title = card?.title ?: match?.title ?: query
        val meta = card?.let(::titleMeta)
            ?: match?.let { listOfNotNull(kindWord(it.kind), it.year?.toString()).joinToString(" · ") }
        val owned = card?.collectionId != null || matches.isNotEmpty() || rows.any { ownedFile(it) != null }
        TitleHead(title, meta?.ifEmpty { null }, card?.posterUrl ?: rows.firstNotNullOfOrNull { it.posterUrl }, owned)
    }

    val summaryLine: String? by lazy {
        (results as? Loadable.Ready)?.value?.let { summary(matches.size, shown.size, it.providers) }
    }
}

private fun bySeason(rows: List<SearchResult>, season: Int?): List<SearchResult> =
    if (season == null) rows else rows.filter { it.parsedSeason == season }

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
    private val pager = ReleasePager(
        viewModelScope,
        shown = { page -> mutable.value.let { s -> filterLanguage(bySeason(page.rows, s.season), s.language).size } },
    ) { page ->
        container.api().search(
            q = query,
            page = page,
            limit = SEARCH_PAGE_SIZE,
            sortBy = sort.sortBy,
            order = sort.order,
            kind = kind.apiKind,
            tmdbId = tmdbId,
        )
    }

    init {
        if (mutable.value.card == null) {
            viewModelScope.launch {
                val card = bestEffort { container.api().searchTitles(query) }
                    ?.also(SearchMemory::keepTitles)
                    ?.firstOrNull { it.tmdbId == tmdbId }
                if (card != null) mutable.update { it.copy(card = card) }
            }
        }
        viewModelScope.launch {
            pager.state.collect { pages -> mutable.update { it.copy(pages = pages) } }
        }
        retry()
    }

    fun retry() = pager.first()

    fun loadMore() = pager.more()

    fun showMore() = pager.more(byViewer = true)

    fun retryMore() = pager.retryMore()

    fun setLanguage(tag: String?) {
        mutable.update { it.copy(language = tag) }
        pager.filtersChanged()
    }

    fun setSeason(season: Int?) {
        mutable.update { it.copy(season = season, language = null) }
        pager.filtersChanged()
    }

    fun grab(r: SearchResult) = grabber.grabOrPlay(r)
}
