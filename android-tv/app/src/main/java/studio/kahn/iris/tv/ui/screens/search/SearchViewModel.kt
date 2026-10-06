package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
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

/** Everything the search screen draws. */
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
    val results: Loadable<SearchPage>? = null,
    val loadingMore: Boolean = false,
    val moreError: UiError? = null,
) {
    val rows: List<SearchResult> get() = results?.valueOrNull?.rows.orEmpty()
    val shown: List<SearchResult> get() = filterLanguage(rows, language)
    val matches: List<LibraryMatch> get() = if (typed.trim() == query) results?.valueOrNull?.matches.orEmpty() else emptyList()
    val titleCards: List<TitleCard>
        get() = titles?.valueOrNull.orEmpty().filter { kind.apiKind == null || it.kind.value == kind.apiKind }
    val counts: Map<Long, Int> get() = releasesByTitle(shown)
    val audio: List<AudioOption> get() = audioOptions(rows, language)
    val showsResults: Boolean get() = !editing && view != SearchViewMode.TITLES && query.isNotEmpty()

    /** "24 releases from 3 trackers · c411 did not answer", once the trackers answered. */
    val summaryLine: String?
        get() = (results as? Loadable.Ready)?.value?.let { summary(it.matches.size, shown.size, it.providers) }
}

/**
 * Search (web `search/SearchScreen.svelte`): what is typed asks TMDB for
 * titles a moment later (the typeahead); the trackers are asked on submit
 * only, by kind and sort, page after page; the audio filter narrows what
 * is loaded. Recent searches are the account's, kept by the server. The
 * view is remembered per device. [autoPlay] (a voice "play X on Iris")
 * grabs the server's first live release once the first page is in.
 */
@OptIn(FlowPreview::class)
class SearchViewModel(
    private val container: AppContainer,
    initialQuery: String?,
    private var autoPlay: Boolean = false,
) : ViewModel() {
    private val mutable = MutableStateFlow(SearchUiState())
    val state: StateFlow<SearchUiState> = mutable.asStateFlow()
    val grabber = Grabber(container, viewModelScope)

    private val typedFlow = MutableStateFlow("")
    private var searchJob: Job? = null
    private var moreJob: Job? = null
    private var recentJob: Job? = null

    init {
        val opened = initialQuery?.trim().orEmpty()
        if (opened.isNotEmpty()) type(opened)
        viewModelScope.launch {
            val kept = container.prefsStore.searchViewMode.first()
            mutable.update { it.copy(view = kept) }
            if (opened.isNotEmpty()) submit(opened)
        }
        viewModelScope.launch {
            typedFlow
                .map { it.trim() }
                .debounce(TYPEAHEAD_MS)
                .distinctUntilChanged()
                .collectLatest { q -> lookUpTitles(q) }
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

    fun setLanguage(tag: String?) = mutable.update { it.copy(language = tag) }

    fun retry() {
        if (mutable.value.query.isNotEmpty()) runSearch()
    }

    fun retryTitles() {
        viewModelScope.launch { lookUpTitles(mutable.value.typed.trim()) }
    }

    /** The end of the list or grid is in sight: the next page, if the trackers have one. */
    fun loadMore() {
        val s = mutable.value
        val page = s.results?.valueOrNull ?: return
        val next = page.next ?: return
        if (s.loadingMore || s.moreError != null || s.results !is Loadable.Ready) return
        val asked = Asked(s.query, s.kind, s.sort)
        mutable.update { it.copy(loadingMore = true) }
        moreJob = viewModelScope.launch {
            try {
                val res = fetch(asked, next)
                SearchMemory.keep(res.results)
                mutable.update { cur ->
                    val ready = cur.results?.valueOrNull ?: return@update cur.copy(loadingMore = false)
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

    fun grab(r: SearchResult) {
        val owned = ownedFile(r)
        if (owned != null) grabber.playOwned(owned) else grabber.run(releaseKey(r), r.grabTarget())
    }

    private fun record(q: String) {
        viewModelScope.launch {
            // A convenience: failing to keep a search is not the search's failure.
            runCatching { container.api().recordSearch(RecordSearchRequest(q)) }
            loadRecent()
        }
    }

    private suspend fun lookUpTitles(q: String) {
        if (q.length < MIN_QUERY) {
            mutable.update { it.copy(titles = null) }
            return
        }
        val previous = mutable.value.titles ?: Loadable.Loading
        if (previous !is Loadable.Ready) mutable.update { it.copy(titles = Loadable.Loading) }
        val next = load(previous) { container.api().searchTitles(q) }
        next.valueOrNull?.let(SearchMemory::keepTitles)
        mutable.update { it.copy(titles = next) }
    }

    private fun runSearch() {
        val s = mutable.value
        val asked = Asked(s.query, s.kind, s.sort)
        searchJob?.cancel()
        moreJob?.cancel()
        mutable.update { it.copy(results = Loadable.Loading, loadingMore = false, moreError = null) }
        searchJob = viewModelScope.launch {
            val next = load(Loadable.Loading) { SearchPage.first(fetch(asked, 1)) }
            next.valueOrNull?.let { SearchMemory.keep(it.rows) }
            mutable.update { it.copy(results = next) }
            next.valueOrNull?.let(::autoPlayFirst)
        }
    }

    private fun autoPlayFirst(page: SearchPage) {
        if (!autoPlay) return
        autoPlay = false
        page.rows.firstOrNull { !isDead(it.seeders) }?.let(::grab)
    }

    private suspend fun fetch(asked: Asked, page: Int): SearchResponse = container.api().search(
        q = asked.q,
        page = page,
        limit = SEARCH_PAGE_SIZE,
        sortBy = asked.sort.sortBy,
        order = asked.sort.order,
        kind = asked.kind.apiKind,
    )

    private data class Asked(val q: String, val kind: SearchKind, val sort: SearchSort)

    private companion object {
        const val TYPEAHEAD_MS = 300L
    }
}
