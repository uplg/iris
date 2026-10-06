package studio.kahn.iris.tv.ui.screens.library

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.ui.format.NO_SUBTITLES
import studio.kahn.iris.tv.ui.format.audioChoiceWords
import studio.kahn.iris.tv.ui.format.subtitleChoiceWords
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.api
import studio.kahn.iris.tv.data.CollectionDetail
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.CreateFollowRequest
import studio.kahn.iris.tv.data.DismissGoneRequest
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.RemoveWatchlistRequest
import studio.kahn.iris.tv.data.ResolveBody
import studio.kahn.iris.tv.data.UpdatePlaybackPrefs
import studio.kahn.iris.tv.data.isVideoPath
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.map
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.format.allWatched
import studio.kahn.iris.tv.ui.format.duration
import studio.kahn.iris.tv.ui.format.formatSize
import studio.kahn.iris.tv.ui.format.languageName
import studio.kahn.iris.tv.ui.format.markedWatchedWords
import studio.kahn.iris.tv.ui.format.recentTime
import studio.kahn.iris.tv.ui.components.Notice
import studio.kahn.iris.tv.ui.state.LiveRead
import studio.kahn.iris.tv.ui.state.FAST_MS

/** One episode row, its words already said. */
@Immutable
data class EpisodeRowUi(
    val key: String,
    val number: String,
    /** The row's own words: TMDB's episode name, else `Episode 4`. */
    val heading: String,
    /** The full name, `Episode 4 · Woe's Hollow`, for the sheet. */
    val title: String,
    val state: RowState,
    /** Offers and reclaimed releases, in words. */
    val details: List<String>,
    val languages: List<String?>,
    val actions: List<EpisodeAction>,
    val overview: String?,
    /** `Aired 2 Oct 2025 · 52 min`. */
    val aired: String?,
    val words: String,
)

@Immutable
data class PackUi(val key: String, val title: String, val facts: String, val season: Long, val language: String?)

@Immutable
data class GoneUi(val infohash: String, val name: String, val watchLine: String?, val watched: Boolean, val facts: String, val provider: String, val externalId: String)

@Immutable
data class FileUi(val key: String, val infohash: String, val fileIdx: Int, val name: String, val facts: String)

@Immutable
data class SeasonUi(val season: Long, val label: String)

@Immutable
data class LanguagesUi(
    val audio: String?,
    val subtitles: String?,
    val forCollection: Boolean,
    /** Codes worth offering first: the releases', the original one, English and French. */
    val audioOptions: List<String>,
    val subtitleOptions: List<String>,
)

/** A title's page, its words already said. */
@Immutable
data class CollectionPage(
    val id: String,
    val title: String,
    val posterUrl: String?,
    val eyebrow: String,
    val facts: String,
    val chips: List<String>,
    val fresh: Int,
    val overview: String?,
    val series: Boolean,
    val playLabel: String,
    val playTarget: PlayTarget?,
    val onWatchlist: Boolean?,
    /** Off the watchlist takes the title's SCENE name; a title without one stays on it. */
    val canLeaveWatchlist: Boolean,
    /** Every file on disk finished: the page's toggle offers to undo it. */
    val watched: Boolean,
    val showEpisodes: Boolean,
    val absolute: Boolean,
    val seasons: List<SeasonUi>,
    val season: Long?,
    val seasonFact: String?,
    val packs: List<PackUi>,
    val episodes: List<EpisodeRowUi>,
    /** Where a long list opens: near the first episode not watched. */
    val opening: Int,
    val emptyEpisodes: String?,
    val onDisk: List<ReleaseRow>,
    val gone: List<GoneUi>,
    val files: List<FileUi>,
    val showFiles: Boolean,
)

@Immutable
data class CollectionUiState(
    val page: Loadable<CollectionPage> = Loadable.Loading,
    val languages: Loadable<LanguagesUi>? = null,
    val busy: Set<String> = emptySet(),
    val notice: Notice? = null,
)

/** Where to go after an action: the player, in place of this page or on top of it. */
@Immutable
data class PlayEvent(val infohash: String, val fileIdx: Int, val replace: Boolean)

@Immutable
private data class CollectionControls(
    val season: Long? = null,
    val busy: Set<String> = emptySet(),
    val notice: Notice? = null,
)

/**
 * A title of the library (web `/collection/[id]`): its head, the episodes of a series (season
 * packs, offers to grab in a chosen language, reclaimed releases to download again), what is
 * on disk, the series' languages, what used to be on disk, and the files themselves.
 * Read again every few seconds while something downloads, slowly otherwise, only while the
 * screen is started. A movie with one copy goes straight to the player.
 */
class CollectionViewModel(private val container: AppContainer, private val collectionId: String) : ViewModel() {
    private val detail = LiveRead({ c: CollectionDetail? -> if (c?.torrents?.any(::moving) == true) FAST_MS else 60_000L }) {
        container.api().collectionDetail(collectionId)
    }
    private val meta = MutableStateFlow<MediaMetadata?>(null)
    private val watching = MutableStateFlow<List<ContinueWatchingItem>>(emptyList())
    private val progress = MutableStateFlow<Map<String, List<FileProgressEntry>>>(emptyMap())
    private val prefs = LiveRead({ _: PlaybackPrefsResponse? -> 10 * 60_000L }) {
        container.api().seriesPlaybackPreferences(collectionId)
    }
    private val controls = MutableStateFlow(CollectionControls())
    private val play = MutableStateFlow<PlayEvent?>(null)
    val playEvents: StateFlow<PlayEvent?> = play
    /** The episode row pressed last: coming back from the player lands on it. */
    var lastRow: String? = null
    private var decided = false
    private var metaFor: Long? = null
    private var prefsAsked = false

    val state: StateFlow<CollectionUiState> = combine(
        detail.state,
        combine(meta, watching, progress, ::Triple),
        prefs.state,
        controls,
    ) { d, (m, cw, p), pr, c ->
        val series = d.valueOrNull?.kind == MediaKind.tv
        CollectionUiState(
            page = d.map { collectionPage(it, m, cw, p, c.season) },
            languages = if (series) pr.map { languagesUi(it, d.valueOrNull, m) } else null,
            busy = c.busy,
            notice = c.notice,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), CollectionUiState())

    /** Reads while the screen is started; the side reads follow each change of the title. */
    suspend fun pollWhileStarted() = coroutineScope {
        launch { readSides() }
        launch { detail.poll() }
        launch {
            detail.state.collect { s ->
                val c = s.valueOrNull ?: return@collect
                if (!decided) {
                    decided = true
                    straightToPlayer(c)?.let { play.value = PlayEvent(it.infohash, it.fileIdx, replace = true) }
                }
                if (c.kind == MediaKind.tv && !prefsAsked) {
                    prefsAsked = true
                    launch { prefs.refresh() }
                }
                val tmdb = c.tmdbId
                if (tmdb != null && tmdb != metaFor) {
                    metaFor = tmdb
                    launch { meta.value = runCatching { container.api().tmdbMetadata(tmdb, c.kind.value) }.getOrNull() }
                }
                val known = progress.value.keys
                if (c.torrents.any { it.infohash !in known }) launch { readProgress(c) }
            }
        }
    }

    fun retry() = detail.poke()

    fun chooseSeason(season: Long) = controls.update { it.copy(season = season) }

    fun consumePlay() {
        play.value = null
    }

    private suspend fun readSides() {
        watching.value = runCatching { container.api().continueWatching() }.getOrDefault(watching.value)
        detail.value?.let { readProgress(it) }
    }

    private suspend fun readProgress(c: CollectionDetail) = coroutineScope {
        val api = container.api()
        val reads = c.torrents.map { t -> async { t.infohash to runCatching { api.torrentProgress(t.infohash) }.getOrDefault(emptyList()) } }
        progress.value = reads.awaitAll().toMap()
    }

    private suspend fun refreshAll() = coroutineScope {
        launch { detail.refresh() }
        launch { readSides() }
    }

    fun grab(a: EpisodeAction.Grab, words: String) = act("grab:${a.season}:${a.episode}:${a.language}", null) {
        val res = container.api().grabCollectionEpisode(collectionId, a.season.toInt(), a.episode.toInt(), a.language)
        refreshAll()
        play.value = PlayEvent(res.infohash, res.fileIdx.toInt(), replace = false)
        "Grabbed $words."
    }

    fun grabPack(p: PackUi, andPlay: Boolean) = act((if (andPlay) "pack-play:" else "pack:") + p.key, null) {
        val res = container.api().grabCollectionEpisode(collectionId, p.season.toInt(), 1, p.language)
        refreshAll()
        if (andPlay) play.value = PlayEvent(res.infohash, res.fileIdx.toInt(), replace = false)
        "${seasonName(p.season)} is downloading."
    }

    fun downloadAgain(g: Variant.Gone) = act("again:${g.infohash}", null) {
        container.api().ingest(ResolveBody(providerId = g.sourceProvider, externalId = g.sourceExternalId, allowDuplicate = true, tmdbId = detail.value?.tmdbId))
        refreshAll()
        play.value = PlayEvent(g.infohash, g.fileIdx, replace = false)
        "${g.releaseName} is downloading again. Your watch position is kept."
    }

    fun downloadAgain(g: GoneUi) = act("again:${g.infohash}", null) {
        container.api().ingest(ResolveBody(providerId = g.provider, externalId = g.externalId, allowDuplicate = true, tmdbId = detail.value?.tmdbId))
        refreshAll()
        "${g.name} is downloading again. Your watch position is kept."
    }

    fun hide(infohash: String, name: String) = act("hide:$infohash", null) {
        container.api().dismissGone(DismissGoneRequest(infohash = infohash))
        detail.refresh()
        "The removed release of $name is hidden. Your history is kept."
    }

    fun markWatched(a: EpisodeAction.MarkWatched, words: String) = act("watched:${a.infohash}:${a.fileIdx}", null) {
        container.api().markWatched(a.infohash, a.fileIdx)
        refreshAll()
        "Marked $words as watched."
    }

    fun toggleWatchlist() = act("watchlist", null) {
        val c = detail.value ?: return@act null
        val listed = c.onWatchlist == true
        val name = c.normalizedName
        when {
            listed && name == null -> return@act null
            listed && name != null -> container.api().removeFromWatchlist(RemoveWatchlistRequest(name))
            else -> container.api().addFollow(CreateFollowRequest(c.displayTitle, c.tmdbId))
        }
        detail.refresh()
        if (listed) "${c.displayTitle} is no longer on your watchlist." else "${c.displayTitle} is on your watchlist."
    }

    fun toggleWatched() = act(WATCHED_KEY, null) {
        val c = detail.value ?: return@act null
        val watched = allWatched(c.watch, c.kind == MediaKind.tv, c.episodes.size.toLong())
        if (watched) container.api().markCollectionUnwatched(collectionId) else container.api().markCollectionWatched(collectionId)
        refreshAll()
        markedWatchedWords(c.displayTitle, watched)
    }

    fun delete(r: ReleaseRow) = act("delete:${r.infohash}", null) {
        container.api().deleteTorrent(r.infohash)
        refreshAll()
        "Deleted ${r.release}."
    }

    fun pause(r: ReleaseRow) = act("pause:${r.infohash}", null) {
        container.api().pauseTorrent(r.infohash)
        detail.refresh()
        "Paused ${r.release}. Its files stay on disk."
    }

    fun resume(r: ReleaseRow) = act("resume:${r.infohash}", null) {
        container.api().resumeTorrent(r.infohash)
        detail.refresh()
        "${r.release} is back in the swarm."
    }

    /** Saves the series' languages; [onSaved] runs once the server answered. */
    fun saveLanguages(audio: String?, subtitles: String?, onSaved: () -> Unit) = act("languages", onSaved) {
        container.api().savePlaybackPreferences(
            UpdatePlaybackPrefs(audioLanguage = audio, subtitleLanguage = subtitles, collectionId = UUID.fromString(collectionId)),
        )
        prefs.refresh()
        val title = detail.value?.displayTitle ?: "this series"
        "Saved for $title: audio ${audioChoiceWords(audio)}, subtitles ${subtitleChoiceWords(subtitles)}."
    }

    private fun act(key: String, onSuccess: (() -> Unit)?, block: suspend CoroutineScope.() -> String?) {
        if (key in controls.value.busy) return
        controls.update { it.copy(busy = it.busy + key, notice = null) }
        viewModelScope.launch {
            var ok = false
            val notice = try {
                val said = coroutineScope { block() }
                ok = true
                said?.let { Notice(it, failed = false) }
            } catch (e: Exception) {
                Notice(e.toUiError().message, failed = true)
            }
            controls.update { it.copy(busy = it.busy - key, notice = notice) }
            if (ok) onSuccess?.invoke()
        }
    }
}

/** The busy key of the title's watched toggle. */
const val WATCHED_KEY = "title-watched"

fun languagesUi(p: PlaybackPrefsResponse, c: CollectionDetail?, m: MediaMetadata?): LanguagesUi {
    val known = releaseCodes(c?.episodes.orEmpty().map { it.language } + c?.availableEpisodes.orEmpty().map { it.language }) +
        listOfNotNull(m?.originalLanguage)
    fun options(current: String?) = (known + listOf("en", "fr") + listOfNotNull(current?.takeIf { it != NO_SUBTITLES })).distinct()
    return LanguagesUi(p.audioLanguage, p.subtitleLanguage, p.forCollection == true, options(p.audioLanguage), options(p.subtitleLanguage))
}

fun collectionPage(
    c: CollectionDetail,
    m: MediaMetadata?,
    watching: List<ContinueWatchingItem>,
    progress: Map<String, List<FileProgressEntry>>,
    chosenSeason: Long?,
    now: Instant = Instant.now(),
): CollectionPage {
    val series = c.kind == MediaKind.tv
    val absolute = c.numbering == "absolute"
    val rows = episodesOf(c)
    val seasons = if (absolute) emptyList() else seasonsOf(rows, c.seasonPacks.orEmpty())
    val season = chosenSeason?.takeIf { s -> seasons.any { it.season == s } } ?: firstSeason(seasons)
    val current = seasons.firstOrNull { it.season == season }
    val shown = if (absolute) rows else current?.items.orEmpty()
    val torrents = c.torrents.associateBy { it.infohash }
    val progressOf = progress.mapValues { (_, v) -> v.associateBy { it.fileIdx.toInt() } }
    val resume = resumeOf(c, watching)
    val watched = watching.groupBy { it.infohash }.mapValues { (_, v) -> v.associateBy { it.fileIdx.toInt() } }
    val eyebrow = listOfNotNull(
        if (series) "Series" else "Movie",
        m?.year?.toString(),
        m?.genres?.takeIf { it.isNotEmpty() }?.take(3)?.joinToString(", "),
    ).joinToString(" · ")
    val showEpisodes = hasEpisodes(c)
    return CollectionPage(
        id = c.id.toString(),
        title = c.displayTitle,
        posterUrl = tmdbPosterUrl(c.posterPath ?: m?.posterPath, "w342"),
        eyebrow = eyebrow,
        facts = heroFacts(c, rows, m?.runtimeMinutes, m?.voteScore),
        chips = heroChips(c, m?.originalLanguage),
        fresh = c.hasNewSinceLastVisit ?: 0,
        overview = m?.overview?.takeIf { it.isNotBlank() },
        series = series,
        playLabel = playLabel(c, resume),
        playTarget = resume?.let { PlayTarget(it.infohash, it.fileIdx.toInt()) } ?: firstPlayable(c),
        onWatchlist = if (series) c.onWatchlist ?: false else null,
        canLeaveWatchlist = c.normalizedName != null,
        watched = allWatched(c.watch, series, c.episodes.size.toLong()),
        showEpisodes = showEpisodes,
        absolute = absolute,
        seasons = if (seasons.size > 1) seasons.map { SeasonUi(it.season, seasonLabel(it)) } else emptyList(),
        season = season,
        seasonFact = shown.takeIf { it.isNotEmpty() }?.let(::episodesFact),
        packs = current?.packs.orEmpty().map { p ->
            PackUi(
                key = "${p.season}-${p.language ?: "_"}-${p.indexerTorrentId}",
                title = "${seasonName(p.season)}: the full season is available in one release",
                facts = packFacts(p),
                season = p.season,
                language = p.language,
            )
        },
        episodes = shown.map { ep -> episodeRow(ep, torrents::get) { ih, idx -> progressOf[ih]?.get(idx) } },
        opening = if (shown.size > 40) openingIndex(shown) else 0,
        emptyEpisodes = when {
            !showEpisodes -> null
            absolute && rows.isEmpty() -> "No episode found yet for this series."
            !absolute && seasons.isEmpty() -> "No episode found yet for this series."
            current != null && current.items.isEmpty() ->
                "No single episode is available on its own yet. The season pack above brings every episode in one go."
            else -> null
        },
        onDisk = c.torrents.map { t ->
            val row = releaseRow(t, c.displayTitle, c.posterPath, watched[t.infohash], now)
            val facts = listOfNotNull(qualityWords(t.name ?: t.infohash), formatSize(t.totalSizeBytes), "added by ${t.addedByName} ${recentTime(t.addedAt, now)}")
                .joinToString(" · ")
            row.copy(
                facts = facts,
                playIdx = if (series) null else mainVideo(t)?.index,
                resume = !series && mainVideo(t)?.let { watchState(watched[t.infohash]?.get(it.index)).resumable } == true,
            )
        },
        gone = goneReleasesBelow(c).map { r ->
            GoneUi(
                infohash = r.infohash,
                name = r.name,
                watchLine = goneWatchLine(r.watched, r.positionSeconds, r.durationSeconds, r.lastWatchedAt, now),
                watched = r.watched == true,
                facts = listOfNotNull(formatSize(r.totalSizeBytes), "via ${r.sourceProvider}", r.deletedAt?.let { "removed ${recentTime(it, now)}" })
                    .joinToString(" · "),
                provider = r.sourceProvider,
                externalId = r.sourceExternalId,
            )
        },
        files = c.torrents.flatMap { t ->
            t.files.filter { isVideoPath(it.path) }.map { f ->
                FileUi(
                    key = "${t.infohash}:${f.index}",
                    infohash = t.infohash,
                    fileIdx = f.index,
                    name = f.path.substringAfterLast('/'),
                    facts = fileFacts(f.sizeBytes, watchState(watched[t.infohash]?.get(f.index))),
                )
            }
        },
        showFiles = !showEpisodes && !(c.kind == MediaKind.movie && c.torrents.size > 1),
    )
}

private val AIRED = java.time.format.DateTimeFormatter.ofPattern("d MMM yyyy", java.util.Locale.UK)

private fun episodeRow(
    ep: Episode,
    torrent: (String) -> studio.kahn.iris.tv.data.TorrentView?,
    progress: (String, Int) -> FileProgressEntry?,
): EpisodeRowUi {
    val state = rowState(ep, torrent, progress)
    val offers = offersByLanguage(ep)
    val gone = ep.variants.filterIsInstance<Variant.Gone>()
    val aired = ep.info?.airDate?.let { runCatching { java.time.LocalDate.parse(it).format(AIRED) }.getOrNull() }
    return EpisodeRowUi(
        key = ep.key,
        number = (ep.absolute ?: ep.episode).toString(),
        heading = ep.info?.name?.takeIf { it.isNotBlank() } ?: episodeName(ep),
        title = episodeTitle(ep),
        state = state,
        details = buildList {
            if (offers.isNotEmpty()) add(offers.joinToString("; ") { offerFacts(it) })
            if (gone.isNotEmpty()) add(gone.joinToString("; ") { goneFacts(it) })
        },
        languages = ep.variants.map { it.language }.distinct(),
        actions = episodeActions(ep, state, torrent),
        overview = ep.info?.overview?.takeIf { it.isNotBlank() },
        aired = listOfNotNull(aired?.let { "Aired $it" }, ep.info?.runtimeMinutes?.takeIf { it > 0 }?.let { duration(it * 60.0) })
            .takeIf { it.isNotEmpty() }?.joinToString(" · "),
        words = episodeWords(ep),
    )
}
