package studio.kahn.iris.tv.ui.screens.library

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import java.time.Instant
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.api
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.DismissGoneRequest
import studio.kahn.iris.tv.data.HomeSummary
import studio.kahn.iris.tv.data.LibraryResponse
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.WatchlistItem
import studio.kahn.iris.tv.data.isVideoPath
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.map
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.format.IN_PROGRESS
import studio.kahn.iris.tv.ui.format.allWatched
import studio.kahn.iris.tv.ui.format.markedWatchedWords
import studio.kahn.iris.tv.ui.format.formatSize
import studio.kahn.iris.tv.ui.format.formatSpeed
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.format.resumeOf
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.Notice
import studio.kahn.iris.tv.ui.state.LiveRead
import studio.kahn.iris.tv.ui.state.FAST_MS
import studio.kahn.iris.tv.ui.state.SLOW_MS

/** The library's two views (web: Titles / Downloads and seeding). */
enum class LibraryView(val label: String) {
    Titles("Titles"),
    Downloads("Downloads and seeding"),
}

/** One poster of the Titles view, its words already said. */
@Immutable
data class TitleCard(
    val id: String,
    val title: String,
    val kind: String,
    val posterUrl: String?,
    val meta: String,
    val status: Status,
    val ghost: Boolean,
    /** Watched share (0..1) of what is in progress; its words are in [status]. */
    val progress: Float?,
    /** Every file on disk finished: what Hold OK offers to undo. */
    val watched: Boolean = false,
)

@Immutable
data class TitlesUi(
    val cards: List<TitleCard>,
    val all: Int,
    val countWords: String,
    val showChoices: List<Pair<ShowFilter, String>>,
    /** The Show filter in force (a state no title is in falls back to everything). */
    val showing: ShowFilter,
)

/** One release of the Downloads view, its words already said. */
@Immutable
data class ReleaseRow(
    val infohash: String,
    val title: String,
    val release: String,
    val collectionId: String?,
    val posterUrl: String?,
    /** Downloaded share (0..1) while it is not finished; its words are in [progressWords]. */
    val progress: Float?,
    val progressWords: String?,
    val status: Status,
    val facts: String,
    /** The one video to play, and whether the caller stopped part-way. */
    val playIdx: Int?,
    val resume: Boolean,
    val videoCount: Int,
    val canDelete: Boolean,
    val noDeleteReason: String,
    val canPause: Boolean,
    val canResume: Boolean,
    val deleteBody: String,
)

@Immutable
data class ReleaseGroupUi(val group: ReleaseGroup, val fact: String, val rows: List<ReleaseRow>)

@Immutable
data class DownloadsUi(
    val groups: List<ReleaseGroupUi>,
    val all: Int,
    val countWords: String,
    val totals: String,
)

@Immutable
data class LibraryUiState(
    val view: LibraryView = LibraryView.Titles,
    val facts: String? = null,
    val filters: TitleFilters = TitleFilters(),
    val releaseQuery: String = "",
    val titles: Loadable<TitlesUi> = Loadable.Loading,
    val downloads: Loadable<DownloadsUi> = Loadable.Loading,
    /** Keys of the actions waiting for the server (`delete:<infohash>`, `hide:<id>`…). */
    val busy: Set<String> = emptySet(),
    val notice: Notice? = null,
)

/** The torrents view of the library, with the lifetime totals. */
@Immutable
data class Torrents(val items: List<TorrentView>, val uploaded: Long, val downloaded: Long?)

@Immutable
private data class Controls(
    val view: LibraryView,
    val filters: TitleFilters,
    val releaseQuery: String,
    val busy: Set<String>,
    val notice: Notice?,
)

private fun List<TorrentView>.anyMoving() = any(::moving)

/**
 * The library (web `/library`): titles as a poster grid with type and state filters and a
 * sort, or every release grouped by what it does, with play, pause or resume, and delete.
 * Reads poll only while the screen is started ([pollWhileStarted]), quick while a download
 * moves; every action waits for the server and reads again before showing a change.
 */
class LibraryViewModel(private val container: AppContainer, initialView: LibraryView?) : ViewModel() {
    private val torrents = LiveRead({ t: Torrents? -> if (t?.items?.anyMoving() == true) FAST_MS else SLOW_MS }) {
        val v = (container.api().library("torrents") as LibraryResponse.TorrentsWrapper).value
        Torrents(v.items, v.totalUploadedBytes, v.totalDownloadedBytes)
    }
    private val collections = LiveRead({ _: List<CollectionListItem>? -> if (torrents.value?.items?.anyMoving() == true) 10_000L else 60_000L }) {
        (container.api().library("collections") as LibraryResponse.CollectionsWrapper).value.items
    }
    private val summary = LiveRead({ s: HomeSummary? -> if ((s?.downloading ?: 0) > 0) FAST_MS else SLOW_MS }) {
        container.api().homeSummary()
    }
    private val watching = LiveRead({ _: List<ContinueWatchingItem>? -> 60_000L }) { container.api().continueWatching() }
    private val watchlist = LiveRead({ _: List<WatchlistItem>? -> 5 * 60_000L }) { container.api().watchlist() }

    private val controls = MutableStateFlow(
        Controls(initialView ?: LibraryView.Titles, TitleFilters(), "", emptySet(), null),
    )
    private var chosen = false

    init {
        if (initialView == null) {
            viewModelScope.launch {
                val kept = container.prefsStore.libraryView.first()
                val view = LibraryView.entries.firstOrNull { it.name == kept } ?: LibraryView.Titles
                if (!chosen) controls.update { it.copy(view = view) }
            }
        }
    }

    /** The title opened last: coming back lands on it. */
    var lastOpened: String? = null

    val state: StateFlow<LibraryUiState> = combine(
        controls,
        collections.state,
        torrents.state,
        combine(summary.state, watching.state, watchlist.state, ::Triple),
    ) { c, cols, tors, (sum, cw, wl) ->
        val torrentList = tors.valueOrNull?.items.orEmpty()
        val counts = cols.valueOrNull?.let(::titleCounts)
        LibraryUiState(
            view = c.view,
            facts = libraryFacts(counts, sum.valueOrNull),
            filters = c.filters,
            releaseQuery = c.releaseQuery,
            titles = cols.map { titlesUi(it, torrentList, wl.valueOrNull.orEmpty(), c.filters) },
            downloads = tors.map { downloadsUi(it, cols.valueOrNull.orEmpty(), cw.valueOrNull.orEmpty(), c.releaseQuery) },
            busy = c.busy,
            notice = c.notice,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), LibraryUiState(view = controls.value.view))

    /** Polls every read; run by the screen only while it is started. */
    suspend fun pollWhileStarted() = coroutineScope {
        launch { torrents.poll() }
        launch { collections.poll() }
        launch { summary.poll() }
        launch { watching.poll() }
        launch { watchlist.poll() }
    }

    fun retry() {
        listOf(torrents, collections, summary, watching, watchlist).forEach { it.poke() }
    }

    /** The view chosen, kept on this device for the next visit. */
    fun choose(view: LibraryView) {
        chosen = true
        controls.update { it.copy(view = view, notice = null) }
        viewModelScope.launch { container.prefsStore.setLibraryView(view.name) }
    }

    fun setFilters(filters: TitleFilters) = controls.update { it.copy(filters = filters) }

    fun setReleaseQuery(query: String) = controls.update { it.copy(releaseQuery = query) }

    fun hide(c: TitleCard) = act("hide:${c.id}", "${c.title} is hidden from your library. Watching it again brings it back.") {
        container.api().dismissGone(DismissGoneRequest(collectionId = java.util.UUID.fromString(c.id)))
        coroutineScope {
            launch { collections.refresh() }
            launch { summary.refresh() }
        }
    }

    fun delete(r: ReleaseRow) = act("delete:${r.infohash}", "Deleted ${r.title}.") {
        container.api().deleteTorrent(r.infohash)
        refreshAll()
    }

    fun pause(r: ReleaseRow) = act("pause:${r.infohash}", "Paused ${r.title}. Its files stay on disk.") {
        container.api().pauseTorrent(r.infohash)
        refreshAll()
    }

    fun resume(r: ReleaseRow) = act("resume:${r.infohash}", "${r.title} is back in the swarm.") {
        container.api().resumeTorrent(r.infohash)
        refreshAll()
    }

    private suspend fun refreshAll() = coroutineScope {
        launch { torrents.refresh() }
        launch { collections.refresh() }
        launch { summary.refresh() }
        launch { watching.refresh() }
    }

    private fun act(key: String, done: String, block: suspend CoroutineScope.() -> Unit) {
        if (key in controls.value.busy) return
        controls.update { it.copy(busy = it.busy + key, notice = null) }
        viewModelScope.launch {
            val notice = try {
                coroutineScope { block() }
                Notice(done, failed = false)
            } catch (e: Exception) {
                Notice(e.toUiError().message, failed = true)
            }
            controls.update { it.copy(busy = it.busy - key, notice = notice) }
        }
    }
}

fun titlesUi(
    items: List<CollectionListItem>,
    torrents: List<TorrentView>,
    watchlist: List<WatchlistItem>,
    filters: TitleFilters,
): TitlesUi {
    val activity = activityByCollection(torrents)
    val choices = showChoices(items, activity)
    val showing = if (choices.any { it.first == filters.show }) filters.show else ShowFilter.All
    val shown = filterTitles(items, filters.copy(show = showing), activity)
    val fresh = watchlist.associate { it.id.toString() to it.newCount }
    val cards = shown.map { c ->
        val id = c.id.toString()
        val base = titleStatus(c, activity[id])
        val inProgress = base.text.startsWith(IN_PROGRESS)
        val news = fresh[id] ?: 0
        val status = if (base.tone == StatusTone.Ok && !inProgress && news > 0) Status(StatusTone.Ok, plural(news, "new episode")) else base
        TitleCard(
            id = id,
            title = c.displayTitle,
            kind = kindOf(c).word,
            posterUrl = tmdbPosterUrl(c.posterPath, "w342"),
            meta = titleMeta(c),
            status = status,
            ghost = c.ghost == true,
            progress = if (inProgress) resumeOf(c.watch)?.share else null,
            watched = allWatched(c.watch, c.kind == MediaKind.tv, c.episodeCount),
        )
    }
    return TitlesUi(cards, items.size, countWords(shown.size, items.size), choices, showing)
}

fun downloadsUi(
    t: Torrents,
    collections: List<CollectionListItem>,
    watching: List<ContinueWatchingItem>,
    query: String,
    now: Instant = Instant.now(),
): DownloadsUi {
    val byId = collections.associateBy { it.id.toString() }
    val watched = watching.groupBy { it.infohash }.mapValues { (_, v) -> v.associateBy { it.fileIdx.toInt() } }
    fun titleOf(x: TorrentView) = releaseTitle(x, x.collectionId?.let { byId[it.toString()] })
    val q = query.trim().lowercase()
    val shown = if (q.isEmpty()) {
        t.items
    } else {
        t.items.filter { x -> listOf(x.name.orEmpty(), x.infohash, x.addedByName, titleOf(x)).any { it.lowercase().contains(q) } }
    }
    val groups = ReleaseGroup.entries.mapNotNull { g ->
        val items = shown.filter { groupOf(it) == g }
        if (items.isEmpty()) return@mapNotNull null
        val n = plural(items.size, "release")
        val fact = when (g) {
            ReleaseGroup.Downloading -> "$n · ${formatSpeed(items.sumOf { it.downloadSpeedBps })} down"
            ReleaseGroup.Seeding -> "$n · ${formatSpeed(items.sumOf { it.uploadSpeedBps })} up"
            ReleaseGroup.Attention -> n
        }
        ReleaseGroupUi(g, fact, items.map { releaseRow(it, titleOf(it), byId[it.collectionId?.toString()]?.posterPath, watched[it.infohash], now) })
    }
    val down = t.items.sumOf { it.downloadSpeedBps }
    val up = t.items.sumOf { it.uploadSpeedBps }
    val ratio = ratioOf(t.uploaded, t.downloaded)
    val totals = listOfNotNull(
        "Right now ${formatSpeed(down)} down",
        "${formatSpeed(up)} up",
        "${t.items.size} active",
        "${formatSize(t.uploaded)} sent in all",
        ratio?.let { "ratio %.2f".format(java.util.Locale.ROOT, it) },
    ).joinToString(" · ")
    val count = if (shown.size == t.items.size) plural(t.items.size, "release") else "Showing ${shown.size} of ${t.items.size} releases"
    return DownloadsUi(groups, t.items.size, count, totals)
}

fun releaseRow(
    t: TorrentView,
    title: String,
    posterPath: String?,
    watched: Map<Int, ContinueWatchingItem>?,
    now: Instant,
): ReleaseRow {
    val videos = t.files.filter { isVideoPath(it.path) }
    val single = videos.singleOrNull()
    val pct = t.progressPct.coerceIn(0.0, 100.0)
    val bar = !t.finished && pct < 100
    return ReleaseRow(
        infohash = t.infohash,
        title = title,
        release = t.name?.let(::releaseName) ?: t.infohash,
        collectionId = t.collectionId?.toString(),
        posterUrl = tmdbPosterUrl(posterPath, "w154"),
        progress = if (bar) (pct / 100).toFloat() else null,
        progressWords = if (bar) "${formatSize(t.progressBytes)} of ${formatSize(t.totalSizeBytes)}" else null,
        status = releaseStatus(t),
        facts = releaseFacts(t, now),
        playIdx = single?.index,
        resume = single != null && watchState(watched?.get(single.index)).resumable,
        videoCount = videos.size,
        canDelete = t.canDelete == true,
        noDeleteReason = noDeleteReason(t),
        canPause = canPause(t),
        canResume = canResume(t),
        deleteBody = deleteDescription(t),
    )
}
