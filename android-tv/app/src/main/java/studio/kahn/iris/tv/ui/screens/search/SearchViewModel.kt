package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.runtime.Immutable
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.api
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.data.LibraryMatch
import studio.kahn.iris.tv.data.ParsedQueryInfo
import studio.kahn.iris.tv.data.ProviderResultMeta
import studio.kahn.iris.tv.data.RecentSearchView
import studio.kahn.iris.tv.data.RecordSearchRequest
import studio.kahn.iris.tv.data.SearchResponse
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.SearchViewMode
import studio.kahn.iris.tv.data.TitleCard
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.toUiError

/** The tracker releases read so far for one search, every page merged. */
@Immutable
data class SearchPage(
    val rows: List<SearchResult>,
    val providers: List<ProviderResultMeta>,
    val matches: List<LibraryMatch>,
    val parsed: ParsedQueryInfo?,
    val pages: Int,
    val next: Int?,
) {
    fun plus(more: SearchResponse): SearchPage = copy(
        rows = mergeResults(rows, more.results),
        providers = more.providers,
        pages = pages + 1,
        next = nextPage(more, pages + 1),
    )

    companion object {
        fun first(res: SearchResponse) = SearchPage(
            rows = mergeResults(emptyList(), res.results),
            providers = res.providers,
            matches = res.libraryMatches,
            parsed = res.parsedQuery,
            pages = 1,
            next = nextPage(res, 1),
        )
    }
}

/** Everything the search screen draws; what it derives is computed once per state. */
@Immutable
data class SearchUiState(
    /** What is in the field. */
    val typed: String = "",
    /** The words the trackers were asked (on submit), trimmed; "" before any search. */
    val query: String = "",
    /** The field and keyboard take the screen (the Titles view always does). */
    val editing: Boolean = true,
    val view: SearchViewMode = SearchViewMode.TITLES,
    val kind: SearchKind = SearchKind.All,
    val sort: SearchSort = SearchSort.BestMatch,
    /** Page-local audio filter (`language_tag`). */
    val language: String? = null,
    val tooShort: Boolean = false,
    val recent: Loadable<List<RecentSearchView>> = Loadable.Loading,
    /** The recent search being forgotten; "" = all of them. */
    val forgetting: String? = null,
    val forgetError: UiError? = null,
    /** TMDB titles for what is typed (the typeahead); null below 2 characters. */
    val titles: Loadable<List<TitleCard>>? = null,
    val pages: ReleasePages = ReleasePages(),
) {
    val results: Loadable<SearchPage>? get() = pages.results
    val loadingMore: Boolean get() = pages.loadingMore
    val moreError: UiError? get() = pages.moreError

    val rows: List<SearchResult> by lazy { results?.valueOrNull?.rows.orEmpty() }
    val shown: List<SearchResult> by lazy { filterLanguage(rows, language) }
    val matches: List<LibraryMatch> by lazy { if (typed.trim() == query) results?.valueOrNull?.matches.orEmpty() else emptyList() }
    val titleCards: List<TitleCard> by lazy {
        titles?.valueOrNull.orEmpty().filter { kind.apiKind == null || it.kind.value == kind.apiKind }
    }
    val counts: Map<Long, Int> by lazy { releasesByTitle(shown) }
    val audio: List<AudioOption> by lazy { audioOptions(rows, language) }
    val showsResults: Boolean get() = !editing && view != SearchViewMode.TITLES && query.isNotEmpty()

    /** "24 releases from 3 trackers · c411 did not answer", once the trackers answered. */
    val summaryLine: String? by lazy {
        (results as? Loadable.Ready)?.value?.let { summary(it.matches.size, shown.size, it.providers) }
    }
}

/**
 * Search (web `search/SearchScreen.svelte`): what is typed asks TMDB for
 * titles a moment later (the typeahead); the trackers are asked on submit
 * only, by kind and sort, page after page ([ReleasePager]); the audio filter
 * narrows what is loaded. Recent searches are the account's, kept by the
 * server. The view is remembered per device; the words, the search and its
 * filters survive the process being killed under the player ([saved]).
 * [autoPlay] (a voice "play X on Iris") grabs the server's first live release
 * once the first page is in, once.
 */
@OptIn(FlowPreview::class)
class SearchViewModel(
    private val container: AppContainer,
    initialQuery: String?,
    autoPlay: Boolean,
    private val saved: SavedStateHandle,
) : ViewModel() {
    private val mutable = MutableStateFlow(SearchUiState())
    val state: StateFlow<SearchUiState> = mutable.asStateFlow()
    val grabber = Grabber(container, viewModelScope)
    private val pager = ReleasePager(viewModelScope, shown = { filterLanguage(it.rows, mutable.value.language).size }) { page ->
        val s = mutable.value
        container.api().search(
            q = s.query,
            page = page,
            limit = SEARCH_PAGE_SIZE,
            sortBy = s.sort.sortBy,
            order = s.sort.order,
            kind = s.kind.apiKind,
        )
    }

    private var autoPlay = autoPlay && saved.get<Boolean>(K_AUTOPLAY_DONE) != true
    private val typedFlow = MutableStateFlow("")
    private val titlesAgain = MutableStateFlow(0)
    private var recentJob: Job? = null

    init {
        val restoredQuery = saved.get<String>(K_QUERY)
        val opened = initialQuery?.trim().orEmpty()
        if (restoredQuery != null) {
            mutable.update {
                it.copy(
                    query = restoredQuery,
                    kind = SearchKind.entries.firstOrNull { k -> k.name == saved.get<String>(K_KIND) } ?: SearchKind.All,
                    sort = SearchSort.of(saved.get<String>(K_SORT)),
                    language = saved.get<String>(K_LANGUAGE),
                    editing = saved.get<Boolean>(K_EDITING) ?: true,
                )
            }
            type(saved.get<String>(K_TYPED).orEmpty())
        } else if (opened.isNotEmpty()) {
            type(opened)
        }
        viewModelScope.launch {
            val kept = container.prefsStore.searchViewMode.first()
            mutable.update { it.copy(view = kept) }
            when {
                restoredQuery != null -> if (restoredQuery.isNotEmpty()) pager.first()
                opened.isNotEmpty() -> submit(opened)
            }
        }
        viewModelScope.launch {
            typedFlow
                .map { it.trim() }
                .debounce(TYPEAHEAD_MS)
                .distinctUntilChanged()
                .combine(titlesAgain) { q, _ -> q }
                .collectLatest { q -> lookUpTitles(q) }
        }
        viewModelScope.launch {
            pager.state.collect { pages -> mutable.update { it.copy(pages = pages) } }
        }
        viewModelScope.launch {
            mutable.collect { s ->
                saved[K_TYPED] = s.typed
                saved[K_QUERY] = s.query
                saved[K_KIND] = s.kind.name
                saved[K_SORT] = s.sort.name
                saved[K_LANGUAGE] = s.language
                saved[K_EDITING] = s.editing
            }
        }
        loadRecent()
    }

    fun type(text: String) {
        mutable.update { it.copy(typed = text, tooShort = false) }
        typedFlow.value = text
    }

    fun key(char: String) = type(mutable.value.typed + char)

    fun backspace() = type(mutable.value.typed.dropLast(1))

    fun clear() = type("")

    fun edit() = mutable.update { it.copy(editing = true) }

    /** Asks the trackers for [words] (the field by default). */
    fun submit(words: String = mutable.value.typed) {
        val q = words.trim()
        if (q.isNotEmpty() && q.length < MIN_QUERY) {
            mutable.update { it.copy(tooShort = true) }
            return
        }
        if (q.isEmpty()) return
        if (q != mutable.value.typed.trim()) type(q)
        mutable.update {
            it.copy(
                query = q,
                language = if (q == it.query) it.language else null,
                editing = it.view == SearchViewMode.TITLES,
                tooShort = false,
            )
        }
        runSearch()
        record(q)
    }

    /** A title picked: it is a search the person made, kept with the recent ones. */
    fun pickedTitle() {
        val q = mutable.value.typed.trim()
        if (q.length >= MIN_QUERY) record(q)
    }

    fun setView(view: SearchViewMode) {
        viewModelScope.launch { container.prefsStore.setSearchViewMode(view) }
        val s = mutable.value
        mutable.update { it.copy(view = view, editing = view == SearchViewMode.TITLES || (it.typed.isBlank() && it.query.isEmpty())) }
        if (view != SearchViewMode.TITLES && s.typed.trim().length >= MIN_QUERY && s.typed.trim() != s.query) submit()
    }

    fun setKind(kind: SearchKind) {
        mutable.update { it.copy(kind = kind, language = null) }
        if (mutable.value.query.isNotEmpty()) runSearch()
    }

    fun setSort(sort: SearchSort) {
        mutable.update { it.copy(sort = sort) }
        if (mutable.value.query.isNotEmpty()) runSearch()
    }

    fun setLanguage(tag: String?) {
        mutable.update { it.copy(language = tag) }
        pager.filtersChanged()
    }

    fun retry() {
        if (mutable.value.query.isNotEmpty()) runSearch()
    }

    /** The typeahead again, for the words in the field now (a newer keystroke wins). */
    fun retryTitles() {
        titlesAgain.value++
    }

    /** The end of the list or grid is in sight: the next page, unless the filter showed nothing new. */
    fun loadMore() = pager.more()

    /** "Show more releases". */
    fun showMore() = pager.more(byViewer = true)

    fun retryMore() = pager.retryMore()

    fun loadRecent() {
        recentJob?.cancel()
        recentJob = viewModelScope.launch {
            val next = load(mutable.value.recent) { container.api().recentSearches() }
            mutable.update { it.copy(recent = next) }
        }
    }

    /** Forgets one recent search, or all of them ([query] null). Shown once the server agreed. */
    fun forget(query: String?) {
        if (mutable.value.forgetting != null) return
        mutable.update { it.copy(forgetting = query.orEmpty(), forgetError = null) }
        viewModelScope.launch {
            val error = try {
                container.api().forgetRecentSearches(query)
                null
            } catch (e: Exception) {
                e.toUiError()
            }
            val next = load(mutable.value.recent) { container.api().recentSearches() }
            mutable.update { it.copy(recent = next, forgetting = null, forgetError = error) }
        }
    }

    fun grab(r: SearchResult) = grabber.grabOrPlay(r)

    private fun record(q: String) {
        viewModelScope.launch {
            // A convenience: failing to keep a search is not the search's failure.
            bestEffort { container.api().recordSearch(RecordSearchRequest(q)) }
            loadRecent()
        }
    }

    /** Titles for [q]: the ones shown belong to these words, never to the previous ones. */
    private suspend fun lookUpTitles(q: String) {
        if (q.length < MIN_QUERY) {
            mutable.update { it.copy(titles = null) }
            return
        }
        mutable.update { it.copy(titles = Loadable.Loading) }
        val next = load(Loadable.Loading) { container.api().searchTitles(q) }
        next.valueOrNull?.let(SearchMemory::keepTitles)
        mutable.update { it.copy(titles = next) }
    }

    private fun runSearch() = pager.first(::autoPlayFirst)

    private fun autoPlayFirst(page: SearchPage) {
        if (!autoPlay) return
        autoPlay = false
        saved[K_AUTOPLAY_DONE] = true
        page.rows.firstOrNull { !isDead(it.seeders) }?.let(::grab)
    }

    private companion object {
        const val TYPEAHEAD_MS = 300L
        const val K_TYPED = "search_typed"
        const val K_QUERY = "search_query"
        const val K_KIND = "search_kind"
        const val K_SORT = "search_sort"
        const val K_LANGUAGE = "search_language"
        const val K_EDITING = "search_editing"
        const val K_AUTOPLAY_DONE = "search_autoplay_done"
    }
}
