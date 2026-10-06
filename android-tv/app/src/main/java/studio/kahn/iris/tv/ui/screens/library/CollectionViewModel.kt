package studio.kahn.iris.tv.ui.screens.library

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.ui.format.NO_SUBTITLES
import studio.kahn.iris.tv.ui.format.audioChoiceWords
import studio.kahn.iris.tv.ui.format.subtitleChoiceWords
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.api
import studio.kahn.iris.tv.data.TmdbMetadataCache
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.data.CollectionDetail
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.CreateFollowRequest
import studio.kahn.iris.tv.data.DismissGoneRequest
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.RemoveWatchlistRequest
import studio.kahn.iris.tv.data.ResolveBody
import studio.kahn.iris.tv.data.isVideoPath
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.screens.player.LanguageChoices
import studio.kahn.iris.tv.ui.screens.player.perTitle
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.map
import studio.kahn.iris.tv.ui.format.allWatched
import studio.kahn.iris.tv.ui.format.duration
import studio.kahn.iris.tv.ui.format.formatSize
import studio.kahn.iris.tv.ui.format.languageName
import studio.kahn.iris.tv.ui.format.markedWatchedWords
import studio.kahn.iris.tv.ui.format.recentTime
import studio.kahn.iris.tv.ui.components.Notice
import studio.kahn.iris.tv.ui.state.LiveRead
import studio.kahn.iris.tv.ui.state.BusyActions
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
    /** What the series chose itself: the panel starts from these, a null one is « your usual choice ». */
    val own: LanguageChoices,
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
    private val prefs = LiveRead({ _: PlaybackPrefsResponse? -> 10 * 60_000L }) {
        container.api().playbackPreferences(collectionId)
    }
    private val season = MutableStateFlow<Long?>(null)
    private val actions = BusyActions(viewModelScope)
    private val play = MutableStateFlow<PlayEvent?>(null)
    val playEvents: StateFlow<PlayEvent?> = play
    /** The episode row pressed last: coming back from the player lands on it. */
    var lastRow: String? = null
    private var decided = false
    private var metaAsking = false
    private var prefsAsking = false

    // Built off the main thread: a long series (1000+ rows) every few seconds while it downloads.
    val state: StateFlow<CollectionUiState> = combine(
        detail.state,
        combine(meta, watching, ::Pair),
        prefs.state,
        season,
        actions.state,
    ) { d, (m, cw), pr, chosen, a ->
        val series = d.valueOrNull?.kind == MediaKind.tv
        CollectionUiState(
            page = d.map { collectionPage(it, m, cw, chosen) },
            // A server older than per-title choices would save them as the account's: no panel there.
            languages = if (series && pr.valueOrNull?.perTitle != false) pr.map { languagesUi(it, d.valueOrNull, m, UUID.fromString(collectionId)) } else null,
            busy = a.busy,
            notice = a.notice,
        )
    }.flowOn(Dispatchers.Default).stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), CollectionUiState())

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
                // Until they are in: a failed or cut read is asked again on the next change or start.
                if (c.kind == MediaKind.tv && prefs.state.value !is Loadable.Ready && !prefsAsking) {
                    prefsAsking = true
                    launch {
                        try {
                            prefs.refresh()
                        } finally {
                            prefsAsking = false
                        }
                    }
                }
                val tmdb = c.tmdbId
                if (tmdb != null && meta.value == null && !metaAsking) {
                    metaAsking = true
                    launch {
                        try {
                            meta.value = TmdbMetadataCache.get(container.api(), tmdb, c.kind.value)
                        } finally {
                            metaAsking = false
                        }
                    }
                }
            }
        }
    }

    fun retry() {
        detail.poke()
        if (detail.value?.kind == MediaKind.tv) viewModelScope.launch { prefs.refresh() }
    }

    fun chooseSeason(season: Long) {
        this.season.value = season
    }

    fun consumePlay() {
        play.value = null
    }

    private suspend fun readSides() {
        bestEffort { container.api().continueWatching() }?.let { watching.value = it }
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
        container.api().savePlaybackPreferences(LanguageChoices(UUID.fromString(collectionId), audio, subtitles).body())
        prefs.refresh()
        val title = detail.value?.displayTitle ?: "this series"
        "Saved for $title: audio ${audio?.let(::audioChoiceWords) ?: USUAL_CHOICE}, " +
            "subtitles ${subtitles?.let(::subtitleChoiceWords) ?: USUAL_CHOICE}."
    }

    private fun act(key: String, onSuccess: (() -> Unit)?, block: suspend CoroutineScope.() -> String?) {
        actions.runSaying(key, onSuccess, block)
    }
}

/** A series field it never chose itself: it inherits the account's (the web's « your usual choice »). */
const val USUAL_CHOICE = "your usual choice"

/** The busy key of the title's watched toggle. */
const val WATCHED_KEY = "title-watched"

fun languagesUi(p: PlaybackPrefsResponse, c: CollectionDetail?, m: MediaMetadata?, collectionId: UUID): LanguagesUi {
    val known = releaseCodes(c?.episodes.orEmpty().map { it.language } + c?.availableEpisodes.orEmpty().map { it.language }) +
        listOfNotNull(m?.originalLanguage)
    fun options(current: String?) = (known + listOf("en", "fr") + listOfNotNull(current?.takeIf { it != NO_SUBTITLES })).distinct()
    val own = LanguageChoices.of(p, collectionId)
    return LanguagesUi(p.audioLanguage, p.subtitleLanguage, p.forCollection == true, own, options(p.audioLanguage), options(p.subtitleLanguage))
}

fun collectionPage(
    c: CollectionDetail,
    m: MediaMetadata?,
    watching: List<ContinueWatchingItem>,
    chosenSeason: Long?,
    now: Instant = Instant.now(),
): CollectionPage {
    val series = c.kind == MediaKind.tv
    val absolute = c.numbering == "absolute"
    val rows = episodesOf(c)
    val seasons = if (absolute) emptyList() else seasonsOf(rows, c.seasonPacks.orEmpty(), c.episodes)
    val season = chosenSeason?.takeIf { s -> seasons.any { it.season == s } } ?: firstSeason(seasons)
    val current = seasons.firstOrNull { it.season == season }
    val shown = if (absolute) rows else current?.items.orEmpty()
    val torrents = c.torrents.associateBy { it.infohash }
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
                key = "${p.season}-${p.language ?: "_"}-${p.indexerProvider}-${p.indexerTorrentId}",
                title = "${seasonName(p.season)}: the full season is available in one release",
                facts = packFacts(p),
                season = p.season,
                language = p.language,
            )
        },
        episodes = shown.map { ep -> episodeRow(ep, torrents::get) },
        opening = if (shown.size > 40) openingIndex(shown) else 0,
        emptyEpisodes = when {
            !showEpisodes -> null
            absolute && rows.isEmpty() -> "No episode found yet for this series."
            !absolute && seasons.isEmpty() -> "No episode found yet for this series."
            current != null && current.items.isEmpty() && current.packOnDisk ->
                "The season pack is on disk. Its episodes are not known one by one yet: play it from its files below."
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
        // A pack the parser never split (episode 0) plays from its files.
        showFiles = (!showEpisodes && !(c.kind == MediaKind.movie && c.torrents.size > 1)) || c.episodes.any { it.episode == 0L },
    )
}

private val AIRED = java.time.format.DateTimeFormatter.ofPattern("d MMM yyyy", java.util.Locale.UK)

private fun episodeRow(
    ep: Episode,
    torrent: (String) -> studio.kahn.iris.tv.data.TorrentView?,
): EpisodeRowUi {
    val state = rowState(ep, torrent)
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
