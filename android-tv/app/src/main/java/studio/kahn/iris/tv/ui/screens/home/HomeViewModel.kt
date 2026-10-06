package studio.kahn.iris.tv.ui.screens.home

import android.os.SystemClock
import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.async
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.absentAs
import studio.kahn.iris.tv.data.api
import studio.kahn.iris.tv.data.TmdbMetadataCache
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.data.libraryCollections
import studio.kahn.iris.tv.data.libraryTorrents
import studio.kahn.iris.tv.ui.state.BusyActions
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.screens.library.moving
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.DismissCwRequest
import studio.kahn.iris.tv.data.DismissRequest
import studio.kahn.iris.tv.data.FeaturedResponse
import studio.kahn.iris.tv.data.ForYou
import studio.kahn.iris.tv.data.HomeSummary
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.PreferencesResponse
import studio.kahn.iris.tv.data.ProgressUpdate
import studio.kahn.iris.tv.data.RemoveWatchlistRequest
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.WatchlistItem
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.map
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.components.Notice
import studio.kahn.iris.tv.ui.state.FAST_MS
import studio.kahn.iris.tv.ui.state.SLOW_MS
import studio.kahn.iris.tv.ui.state.pollWhile

/** Where an action leads; the screen navigates. */
sealed interface HomeEvent {
    data class Play(val infohash: String, val fileIdx: Int) : HomeEvent
    data class OpenCollection(val id: String) : HomeEvent
    data class Search(val query: String) : HomeEvent
    data object OpenLibrary : HomeEvent
}

/** Everything the home draws, in words. */
@Immutable
data class HomeUiState(
    val hero: HeroModel? = null,
    /** True until the home knows what its hero is (or that it has none). */
    val heroPending: Boolean = true,
    val rightNow: Loadable<List<String>> = Loadable.Loading,
    val continueWatching: Loadable<List<CardModel>> = Loadable.Loading,
    val watchlist: Loadable<List<CardModel>> = Loadable.Loading,
    val watchlistCount: Int = 0,
    val forYou: List<ShelfModel> = emptyList(),
    val library: Loadable<List<CardModel>> = Loadable.Loading,
    val libraryCount: Int = 0,
    /** The action in flight (`get:<tile>`, `remove:<card>`…): one at a time. */
    val busy: String? = null,
    val notice: Notice? = null,
    /** Non-null while the first-run preferences sheet is due. */
    val onboarding: PreferencesResponse? = null,
)

/** A TMDB lookup's identity: the movie and tv namespaces share ids. */
internal data class MetaKey(val id: Long, val kind: MediaKind?)

/** What the home read, as read: [homeUi] turns it into words. */
@Immutable
internal data class HomeData(
    val continueWatching: Loadable<List<ContinueWatchingItem>> = Loadable.Loading,
    val watchlist: Loadable<List<WatchlistItem>> = Loadable.Loading,
    val forYou: Loadable<ForYou> = Loadable.Loading,
    val collections: Loadable<List<CollectionListItem>> = Loadable.Loading,
    val torrents: List<TorrentView> = emptyList(),
    /** Null when the server has no such read (one older than this app). */
    val summary: Loadable<HomeSummary?> = Loadable.Loading,
    val featured: Loadable<FeaturedResponse>? = null,
    val meta: Map<MetaKey, MediaMetadata> = emptyMap(),
    val heroPrefs: Pair<String, PlaybackPrefsResponse>? = null,
    val preferences: PreferencesResponse? = null,
    val onboardingClosed: Boolean = false,
    val busy: String? = null,
    val notice: Notice? = null,
)

private const val LIBRARY_ROW = 12

internal fun ContinueWatchingItem.metaKey(): MetaKey? = tmdbId?.takeIf { tmdbVerified }?.let { MetaKey(it, kind) }
internal fun CollectionListItem.metaKey(): MetaKey? = tmdbId?.let { MetaKey(it, kind) }
private fun FeaturedResponse.pick() = movies.firstOrNull() ?: series.firstOrNull()

/** The home's words from what it read. Pure: the unit tests feed it. */
internal fun homeUi(d: HomeData): HomeUiState {
    val downloads = downloadsByCollection(d.torrents)
    val resume = d.continueWatching.valueOrNull?.firstOrNull()
    val libraryPick = d.collections.valueOrNull?.firstOrNull { it.ghost != true }
    val cwKnown = d.continueWatching !is Loadable.Loading
    val featuredPick = d.featured?.valueOrNull?.pick()
    val hero = when {
        resume != null -> resumeHero(
            resume,
            resume.metaKey()?.let(d.meta::get),
            d.heroPrefs?.takeIf { it.first == heroPrefsKey(resume) }?.second,
        )
        !cwKnown -> null
        libraryPick != null -> libraryHero(libraryPick, libraryPick.metaKey()?.let(d.meta::get))
        featuredPick != null -> featuredHero(featuredPick, featuredMetaKey(featuredPick)?.let(d.meta::get))
        else -> null
    }
    val heroPending = hero == null && (
        !cwKnown ||
            (d.continueWatching.valueOrNull.isNullOrEmpty() && d.collections is Loadable.Loading) ||
            d.featured is Loadable.Loading
        )
    val collections = d.collections.valueOrNull.orEmpty()
    return HomeUiState(
        hero = hero,
        heroPending = heroPending,
        rightNow = d.summary.map { it?.let(::rightNow).orEmpty() },
        continueWatching = d.continueWatching.map { list -> list.map { continueCard(it, it.metaKey()?.let(d.meta::get)) } },
        watchlist = d.watchlist.map { list ->
            list.sortedByDescending { it.newCount }.map { watchlistCard(it, downloads[it.id.toString()]) }
        },
        watchlistCount = d.watchlist.valueOrNull?.size ?: 0,
        forYou = d.forYou.valueOrNull?.let { shelves(it.shelves) }.orEmpty(),
        library = d.collections.map { list -> list.take(LIBRARY_ROW).map { libraryCard(it, downloads[it.id.toString()]) } },
        libraryCount = collections.size,
        busy = d.busy,
        notice = d.notice,
        onboarding = d.preferences?.takeIf { !it.onboardingCompleted && !d.onboardingClosed },
    )
}

internal fun heroPrefsKey(item: ContinueWatchingItem): String = item.collectionId?.toString() ?: ""

// Only the server's title match, which it sets when it trusts it: the tracker's raw id is never shown.
internal fun featuredMetaKey(r: studio.kahn.iris.tv.data.SearchResult): MetaKey? =
    r.titleMatch?.let { MetaKey(it.tmdbId, it.kind) }

/**
 * The home (TV.dc.html): reads every row when the screen starts (so coming back from the
 * player shows where one stopped), then, while it stays started, the live facts and the
 * downloads: every 3 s while something downloads, every 30 s otherwise (web `FAST`/`SLOW`).
 * Every action goes to the server first; rows are read again on its answer.
 */
class HomeViewModel(
    private val container: AppContainer,
    private val now: () -> Long = SystemClock::elapsedRealtime,
) : ViewModel() {
    private val data = MutableStateFlow(HomeData())
    val state: StateFlow<HomeUiState> = data.map(::homeUi)
        .flowOn(Dispatchers.Default)
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), homeUi(data.value))

    private val eventChannel = Channel<HomeEvent>(Channel.BUFFERED)
    val events: Flow<HomeEvent> = eventChannel.receiveAsFlow()

    private val metaInFlight = mutableSetOf<MetaKey>()
    private var forYouReadAt: Long? = null
    private var collectionsReadAt: Long? = null
    private val actions = BusyActions(viewModelScope, oneAtATime = true)

    init {
        viewModelScope.launch {
            actions.state.collect { a -> data.update { it.copy(busy = a.busyKey, notice = a.notice) } }
        }
    }

    /** Run by the screen while it is started: one full read, then the live loop. */
    suspend fun refreshWhileStarted() {
        // The last action's words belong to the visit they were said in (web notices pass).
        if (data.value.busy == null) actions.say(null)
        refreshRows()
        pollWhile({ if (somethingMoves()) FAST_MS else SLOW_MS }) { refreshLive() }
    }

    fun retry() {
        viewModelScope.launch { refreshRows(force = true) }
    }

    private fun olderThan(at: Long?, ttlMs: Long): Boolean = at == null || now() - at >= ttlMs

    private fun somethingMoves(): Boolean =
        data.value.torrents.any(::moving) || (data.value.summary.valueOrNull?.downloading ?: 0) > 0

    private suspend fun refreshRows(force: Boolean = false) = coroutineScope {
        launch { readContinueWatching() }
        launch { readWatchlist() }
        launch { readCollections() }
        launch { readPreferences() }
        if (force || olderThan(forYouReadAt, FOR_YOU_STALE_MS)) launch { readForYou() }
    }

    private suspend fun refreshLive() = coroutineScope {
        val summary = async { load(data.value.summary) { absentAs(null) { container.api().homeSummary() } } }
        val torrents = async { bestEffort { container.api().libraryTorrents().items } }
        val s = summary.await()
        val t = torrents.await()
        data.update { it.copy(summary = s, torrents = t ?: it.torrents) }
        val every = if (somethingMoves()) COLLECTIONS_MOVING_MS else COLLECTIONS_IDLE_MS
        if (olderThan(collectionsReadAt, every)) readCollections()
    }

    private suspend fun readContinueWatching() {
        val next = load(data.value.continueWatching) { container.api().continueWatching(includeGrabbable = true) }
        data.update { it.copy(continueWatching = next) }
        next.valueOrNull?.let { list ->
            list.mapNotNull { it.metaKey() }.forEach(::fetchMeta)
            list.firstOrNull()?.let(::fetchHeroPrefs)
        }
        maybeFeatured()
    }

    private suspend fun readWatchlist() {
        val next = load(data.value.watchlist) { container.api().watchlist() }
        data.update { it.copy(watchlist = next) }
    }

    private suspend fun readForYou() {
        val next = load(data.value.forYou) { container.api().forYou() }
        if (next is Loadable.Ready) forYouReadAt = now()
        data.update { it.copy(forYou = next) }
    }

    private suspend fun readCollections() {
        collectionsReadAt = now()
        val next = load(data.value.collections) { container.api().libraryCollections().items }
        data.update { it.copy(collections = next) }
        next.valueOrNull?.firstOrNull { it.ghost != true }?.metaKey()?.let(::fetchMeta)
        maybeFeatured()
    }

    private suspend fun readPreferences() {
        val prefs = bestEffort { container.api().preferences() } ?: return
        data.update { it.copy(preferences = prefs) }
    }

    // The tracker is asked only when there is nothing of one's own to show.
    private fun maybeFeatured() {
        val d = data.value
        val ownNothing = d.continueWatching.valueOrNull?.isEmpty() == true &&
            d.collections.valueOrNull?.none { it.ghost != true } == true
        // A failed read is asked again (on the next read of the rows, on Retry).
        if (!ownNothing || (d.featured != null && d.featured !is Loadable.Failed)) return
        data.update { it.copy(featured = Loadable.Loading) }
        viewModelScope.launch {
            val next = load(Loadable.Loading) { container.api().discoverFeatured() }
            data.update { it.copy(featured = next) }
            next.valueOrNull?.pick()?.let(::featuredMetaKey)?.let(::fetchMeta)
        }
    }

    private fun fetchMeta(key: MetaKey) {
        if (key in data.value.meta || !metaInFlight.add(key)) return
        viewModelScope.launch {
            val md = try {
                TmdbMetadataCache.get(container.api(), key.id, key.kind?.value)
            } finally {
                metaInFlight.remove(key)
            }
            if (md != null) data.update { it.copy(meta = it.meta + (key to md)) }
        }
    }

    private fun fetchHeroPrefs(item: ContinueWatchingItem) {
        val key = heroPrefsKey(item)
        viewModelScope.launch {
            val prefs = bestEffort {
                val api = container.api()
                api.playbackPreferences(item.collectionId?.toString())
            } ?: return@launch
            data.update { it.copy(heroPrefs = key to prefs) }
        }
    }

    /** The onboarding sheet closed: [saved] are the server's preferences, null when it was put off for this visit. */
    fun onboardingClosed(saved: PreferencesResponse?) {
        data.update { it.copy(preferences = saved ?: it.preferences, onboardingClosed = true) }
        if (saved != null) viewModelScope.launch { readForYou() }
    }

    fun onHeroAction(action: HeroAction) {
        val d = data.value
        val resume = d.continueWatching.valueOrNull?.firstOrNull()
        when (action) {
            HeroAction.Resume, HeroAction.Play -> resume?.let(::play)
            HeroAction.GetAndPlay -> resume?.let(::getAndPlay)
            HeroAction.StartOver -> resume?.let(::startOver)
            HeroAction.AllEpisodes -> resume?.collectionId?.let { emit(HomeEvent.OpenCollection(it.toString())) }
            HeroAction.Open -> d.collections.valueOrNull?.firstOrNull { it.ghost != true }
                ?.let { emit(HomeEvent.OpenCollection(it.id.toString())) }
            HeroAction.FindReleases -> d.featured?.valueOrNull?.pick()?.let { r ->
                emit(HomeEvent.Search(featuredTitle(r, featuredMetaKey(r)?.let(d.meta::get))))
            }
        }
    }

    fun onCardAction(key: String, action: CardAction) {
        val d = data.value
        when {
            key.startsWith(CW_PREFIX) -> {
                val item = d.continueWatching.valueOrNull?.firstOrNull { CW_PREFIX + tileKey(it) == key } ?: return
                when (action) {
                    CardAction.Play -> play(item)
                    CardAction.GetAndPlay -> getAndPlay(item)
                    CardAction.StartOver -> startOver(item)
                    CardAction.OpenSeries -> item.collectionId?.let { emit(HomeEvent.OpenCollection(it.toString())) }
                    CardAction.MarkWatched -> markWatched(key, item)
                    CardAction.RemoveFromContinue -> removeTile(key, item)
                    else -> Unit
                }
            }
            key.startsWith(WATCHLIST_PREFIX) -> {
                val item = d.watchlist.valueOrNull?.firstOrNull { WATCHLIST_PREFIX + it.id == key } ?: return
                when (action) {
                    CardAction.Open -> emit(HomeEvent.OpenCollection(item.id.toString()))
                    CardAction.RemoveFromWatchlist -> leaveWatchlist(key, item)
                    else -> Unit
                }
            }
            key.startsWith(LIBRARY_PREFIX) -> emit(HomeEvent.OpenCollection(key.removePrefix(LIBRARY_PREFIX)))
            else -> {
                val card = d.forYou.valueOrNull?.shelves?.firstNotNullOfOrNull { shelf ->
                    shelf.items.firstOrNull { "fy:${shelf.key}:${it.catalogId}" == key }
                } ?: return
                when (action) {
                    CardAction.FindReleases -> emit(HomeEvent.Search(card.title))
                    CardAction.NotInterested -> act(key) {
                        container.api().dismissForYou(DismissRequest(card.catalogId))
                        readForYou()
                        Notice("${card.title} hidden from your suggestions")
                    }
                    else -> Unit
                }
            }
        }
    }

    private fun play(item: ContinueWatchingItem) {
        if (item.grabbable) return getAndPlay(item)
        emit(HomeEvent.Play(item.infohash, item.fileIdx.toInt()))
    }

    /** Gets the tile's episode in the series' usual language, then plays it while it downloads. */
    private fun getAndPlay(item: ContinueWatchingItem) {
        val cid = item.collectionId
        val season = item.season
        val episode = item.episode
        if (cid == null || season == null || episode == null) {
            emit(cid?.let { HomeEvent.OpenCollection(it.toString()) } ?: HomeEvent.OpenLibrary)
            return
        }
        actions.run("get:${tileKey(item)}") {
            val got = try {
                container.api().grabCollectionEpisode(cid.toString(), season.toInt(), episode.toInt(), "auto")
            } catch (e: Exception) {
                val error = e.toUiError()
                return@run Notice("Could not get ${nextName(item)}. ${error.message} Open the series to pick another release.", StatusTone.Down)
            }
            emit(HomeEvent.Play(got.infohash, got.fileIdx.toInt()))
            launch { readContinueWatching() }
            null
        }
    }

    /** Saves the position back to the start (a deliberate seek, so the server keeps it), then plays. */
    private fun startOver(item: ContinueWatchingItem) = act("over:${tileKey(item)}") {
        container.api().saveProgress(
            item.infohash,
            item.fileIdx.toInt(),
            ProgressUpdate(positionSeconds = 0.0, durationSeconds = item.durationSeconds, seek = true),
        )
        emit(HomeEvent.Play(item.infohash, item.fileIdx.toInt()))
        null
    }

    private fun markWatched(key: String, item: ContinueWatchingItem) = act(key) {
        container.api().markWatched(item.infohash, item.fileIdx.toInt())
        readContinueWatching()
        Notice("${titleOf(item)} marked as watched")
    }

    /** A series leaves the row as a whole (until a newer episode plays), a movie by its file. */
    private fun removeTile(key: String, item: ContinueWatchingItem) = act(key) {
        val body = item.collectionId?.let { DismissCwRequest(collectionId = it) }
            ?: DismissCwRequest(infohash = item.infohash, fileIdx = item.fileIdx)
        container.api().dismissContinueWatching(body)
        readContinueWatching()
        Notice("${titleOf(item)} removed from Continue watching")
    }

    private fun leaveWatchlist(key: String, item: WatchlistItem) = act(key) {
        container.api().removeFromWatchlist(RemoveWatchlistRequest(item.normalizedName))
        readWatchlist()
        refreshLive()
        Notice("${item.name} left your watchlist. It comes back when you get or play one of its episodes.")
    }

    private fun titleOf(item: ContinueWatchingItem): String =
        continueTitle(item, item.metaKey()?.let(data.value.meta::get))

    /** One action at a time: [busy] while it travels, then its notice (or the error, said). */
    private fun act(key: String, block: suspend () -> Notice?) {
        actions.run(key) { block() }
    }

    private fun emit(event: HomeEvent) {
        eventChannel.trySend(event)
    }

    internal companion object {
        const val FOR_YOU_STALE_MS = 60_000L
        const val COLLECTIONS_MOVING_MS = 10_000L
        const val COLLECTIONS_IDLE_MS = 60_000L
    }
}
