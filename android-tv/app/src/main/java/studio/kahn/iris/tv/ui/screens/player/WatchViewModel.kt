package studio.kahn.iris.tv.ui.screens.player

import studio.kahn.iris.tv.ui.state.Reclaimed
import studio.kahn.iris.tv.ui.state.Regrabbed
import studio.kahn.iris.tv.ui.state.SearchInsteadEvent
import studio.kahn.iris.tv.ui.state.regrab
import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import retrofit2.HttpException
import studio.kahn.iris.tv.ui.format.isResumable
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CollectionDetail
import studio.kahn.iris.tv.data.EpisodeContext
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.MediaProbe
import studio.kahn.iris.tv.data.PlayStatus
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.data.irisError
import studio.kahn.iris.tv.data.isVideoPath
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.SLOW_MS
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.pollWhile
import studio.kahn.iris.tv.ui.state.pollWhileStarted
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.format.episodeCode

/** What the player starts from, read once per file before the player is built. */
@Immutable
data class WatchSetup(
    val serverUrl: String,
    val probe: MediaProbe,
    val resumeSec: Double,
    /** This file's saved picks (probe stream indices; subtitle -1 = turned off). */
    val savedAudioIdx: Int?,
    val savedSubIdx: Int?,
    /** The series' (or the account's) preferred languages. */
    val prefAudioLang: String?,
    val prefSubLang: String?,
    /** The series the file belongs to: track picks are kept for it. */
    val collectionId: UUID?,
    /** What that scope chose itself: a pick saves only these, never the account's copied in. */
    val ownLanguages: LanguageChoices = LanguageChoices(collectionId),
)

/** The words above the picture: the title, the episode line, the poster for getting ready. */
@Immutable
data class WatchHeader(
    val title: String,
    val episode: String?,
    val posterUrl: String?,
    /** No episode taxonomy: no next episode, a movie-worded ending. */
    val isMovie: Boolean,
)

/**
 * One file, watched (WatchScreen): the reads and polls behind the player.
 *
 * - [torrent]: the 2 s torrent poll, only while the screen is started. The
 *   player and its controls never read it directly: they read the narrow
 *   flows derived from it ([facts], [fileSizeBytes]), so a poll that changes
 *   nothing on screen recomposes nothing.
 * - [probe] / [setup]: the probe, retried while the head downloads (the
 *   server answers "not yet on disk"), then the saved position, this file's
 *   track picks and the series' preferred languages, committed together so
 *   the player is built once with the right start.
 * - Episode context, the collection and per-file progress for the episodes
 *   panel and the next-episode rule.
 */
class WatchViewModel(
    private val container: AppContainer,
    val infohash: String,
    val fileIdx: Int,
) : ViewModel() {
    private val serverUrl = MutableStateFlow<String?>(null)

    /** The last position played, kept across an activity recreated mid-film (the player resumes there). */
    var playedMs: Long? = null

    // A finished torrent changes little (a season pack's file list is a big read): slow down.
    val torrent: StateFlow<Loadable<TorrentView>> = viewModelScope.pollWhileStarted({ t: TorrentView? ->
        if (t?.finished == true) SLOW_MS else TORRENT_POLL_MS
    }) {
        api().getTorrent(infohash)
    }

    private val probeState = MutableStateFlow<ProbePhase>(ProbePhase.Waiting())
    val probe: StateFlow<ProbePhase> = probeState.asStateFlow()

    private val setupState = MutableStateFlow<WatchSetup?>(null)
    val setup: StateFlow<WatchSetup?> = setupState.asStateFlow()

    private val contextState = MutableStateFlow<EpisodeContext?>(null)
    val episodeContext: StateFlow<EpisodeContext?> = contextState.asStateFlow()

    private val collectionState = MutableStateFlow<CollectionDetail?>(null)
    val collection: StateFlow<CollectionDetail?> = collectionState.asStateFlow()

    private val progressState = MutableStateFlow<Map<Int, FileProgressEntry>>(emptyMap())
    val progressByFile: StateFlow<Map<Int, FileProgressEntry>> = progressState.asStateFlow()

    private val noticeState = MutableStateFlow<String?>(null)
    /** A short message said over the player ("S2:E5 is being prepared."). */
    val notice: StateFlow<String?> = noticeState.asStateFlow()

    private val busyState = MutableStateFlow<String?>(null)
    /** The key of the action in flight: "regrab", "replace", "prepare" or a side row's key. */
    val busy: StateFlow<String?> = busyState.asStateFlow()

    private val regrabFailed = MutableStateFlow(false)
    val regrabError: StateFlow<Boolean> = regrabFailed.asStateFlow()
    val searchInstead = SearchInsteadEvent()

    val header: StateFlow<WatchHeader> = combine(torrent, collectionState, contextState) { t, c, ctx ->
        val file = t.valueOrNull?.files?.firstOrNull { it.index == fileIdx }
        WatchHeader(
            title = c?.displayTitle ?: prettyName(file?.path) ?: prettyName(t.valueOrNull?.name) ?: "Now playing",
            episode = episodeLine(ctx?.current),
            posterUrl = tmdbPosterUrl(c?.posterPath),
            isMovie = ctx?.current == null,
        )
    }.distinctUntilChanged().stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), WatchHeader("", null, null, isMovie = true))

    /** "Playing from disk · 1080p HEVC": the top bar's facts. */
    val facts: StateFlow<String> = combine(torrent, setupState) { t, s ->
        val v = s?.probe?.video?.firstOrNull()
        factsLine(t.valueOrNull, pictureWords(v?.height, v?.codec, v?.hdr?.value))
    }.distinctUntilChanged().stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), "")

    /** The playing file's size, for the seek hint's byte estimate. */
    val fileSizeBytes: StateFlow<Long> = torrent
        .map { t -> t.valueOrNull?.files?.firstOrNull { it.index == fileIdx }?.sizeBytes ?: 0L }
        .distinctUntilChanged()
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), 0L)

    /** The rows of the episodes panel (the season, or the torrent's files). */
    val sideRows: StateFlow<List<SideRow>> = combine(torrent, collectionState, progressState) { t, c, p ->
        val view = t.valueOrNull
        sideRows(
            SideInput(
                infohash = infohash,
                fileIdx = fileIdx,
                isTvCollection = c != null && c.kind == MediaKind.tv,
                episodes = c?.episodes.orEmpty(),
                available = c?.availableEpisodes.orEmpty(),
                videoFiles = view?.files.orEmpty().filter { isVideoPath(it.path) },
                progressByFile = p,
                names = c?.episodeInfo.orEmpty()
                    .mapNotNull { info -> info.name?.takeIf(String::isNotBlank)?.let { (info.season to info.episode) to it } }
                    .toMap(),
            ),
        )
    }.distinctUntilChanged().stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), emptyList())

    val sideTitle: StateFlow<String> = collectionState
        .map { c -> if (c != null && c.kind == MediaKind.tv && (c.episodes.isNotEmpty() || !c.availableEpisodes.isNullOrEmpty())) "Episodes" else "Other files" }
        .distinctUntilChanged()
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), "Other files")

    private var setupJob: Job? = null

    init {
        viewModelScope.launch {
            serverUrl.value = container.sessionStore.serverUrl.first()
            loadSetup()
            launch { refreshProgress() }
            loadSeries()
        }
    }

    /**
     * The episode's context and its series, for the episode wording, the panel and the next
     * episode; a failed read is read again on "Try again" and when the panel opens.
     */
    private suspend fun loadSeries() = coroutineScope {
        if (contextState.value == null) {
            launch { bestEffort { api().episodeContext(infohash, fileIdx) }?.let { contextState.value = it } }
        }
        if (collectionState.value == null) {
            // The collection id rides on the torrent: the first answer is enough.
            val cid = torrent.first { it.valueOrNull != null }.valueOrNull?.collectionId
            if (cid != null) bestEffort { api().collectionDetail(cid.toString()) }?.let { collectionState.value = it }
        }
    }

    private suspend fun api(): IrisApi {
        val url = serverUrl.value ?: container.sessionStore.serverUrl.first()?.also { serverUrl.value = it }
            ?: throw IllegalStateException("Not signed in")
        return container.apiFor(url)
    }

    /** Probe again (Try again, after a regrab). */
    fun retry() {
        loadSetup()
        viewModelScope.launch { loadSeries() }
    }

    private fun loadSetup() {
        setupJob?.cancel()
        setupState.value = null
        probeState.value = ProbePhase.Waiting()
        setupJob = viewModelScope.launch { probeUntilReady() }
    }

    /**
     * Right after a grab the file is not on disk yet: the server answers 400
     * "file not yet on disk" while librqbit fetches the first pieces, so retry
     * every 2 s for about two minutes. 401 and 404 stop at once. Probe,
     * progress, picks and preferences are read into locals and committed
     * together: the player reads the start position once, at build time.
     */
    private suspend fun probeUntilReady() {
        val url = serverUrl.value ?: container.sessionStore.serverUrl.first()
        if (url == null) {
            probeState.value = ProbePhase.Failed("This TV is signed out. Pair it again from Settings.")
            return
        }
        serverUrl.value = url
        val api = container.apiFor(url)
        val collectionId = bestEffort { api.getTorrent(infohash).collectionId }
        var attempts = 0
        while (attempts < PROBE_ATTEMPTS) {
            try {
                val freshProbe = api.probe(infohash, fileIdx)
                val progresses = bestEffort { api.torrentProgress(infohash) }.orEmpty()
                val resume = progresses.firstOrNull { it.fileIdx == fileIdx.toLong() }
                    ?.takeIf { isResumable(it.positionSeconds, it.completed) }?.positionSeconds ?: 0.0
                val saved = bestEffort { api.getProgress(infohash, fileIdx) }
                val prefs = bestEffort { api.playbackPreferences(collectionId?.toString()) }
                setupState.value = WatchSetup(
                    serverUrl = url,
                    probe = freshProbe,
                    resumeSec = resume,
                    savedAudioIdx = saved?.audioTrackIdx?.toInt(),
                    savedSubIdx = saved?.subtitleTrackIdx?.toInt(),
                    prefAudioLang = prefs?.audioLanguage,
                    prefSubLang = prefs?.subtitleLanguage,
                    collectionId = collectionId,
                    ownLanguages = LanguageChoices.of(prefs, collectionId),
                )
                probeState.value = ProbePhase.Done
                return
            } catch (e: HttpException) {
                when (e.code()) {
                    404 -> {
                        probeState.value = ProbePhase.Gone
                        return
                    }
                    401 -> {
                        probeState.value = ProbePhase.Failed(e.toUiError().message)
                        return
                    }
                }
                val message = e.irisError()?.message
                attempts++
                probeState.value = ProbePhase.Waiting(
                    notOnDisk = message?.contains("not yet on disk", ignoreCase = true) ?: true,
                    lastError = message,
                )
                delay(PROBE_RETRY_MS)
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                attempts++
                delay(PROBE_RETRY_MS)
            }
        }
        probeState.value = ProbePhase.Failed("The file did not download far enough to be read. Try again in a moment.")
    }

    /**
     * Polls `/play/status` every 1.5 s (the web's cadence) while [keepGoing]
     * (the player has not painted its first frame yet). Run by the player
     * while the server builds its stream.
     */
    suspend fun pollPlayStatus(keepGoing: () -> Boolean, onStatus: (PlayStatus) -> Unit) {
        val api = api()
        pollWhile({ PLAY_STATUS_POLL_MS }, keepGoing) {
            bestEffort { api.playStatus(infohash, fileIdx) }?.let(onStatus)
        }
    }

    fun refreshProgress() {
        viewModelScope.launch {
            launch { loadSeries() }
            val list = bestEffort { api().torrentProgress(infohash) } ?: return@launch
            progressState.value = list.associateBy { it.fileIdx.toInt() }
        }
    }

    /**
     * "Grab it again": the release from its provenance ([regrab]; same infohash, the saved
     * position applies). Refused, the search for another release opens instead
     * ([searchInstead]); no answer at all, the screen says it could not.
     */
    fun regrab() {
        if (busyState.value != null) return
        busyState.value = BUSY_REGRAB
        viewModelScope.launch {
            val r = bestEffort { regrab(api(), reclaimed()) }
            busyState.value = null
            regrabFailed.value = r == null
            when (r) {
                is Regrabbed.Back -> loadSetup()
                is Regrabbed.Refused -> searchInstead.send(r.search)
                null -> Unit
            }
        }
    }

    /**
     * This release and its words to search by. A reclaimed one is no longer a torrent the
     * server answers for (its title and name ride on that read): the person's history still
     * knows them.
     */
    private suspend fun reclaimed(): Reclaimed {
        val c = collectionState.value
        val name = torrent.value.valueOrNull?.name
        if (c == null && name == null) {
            val seen = bestEffort { api().history(limit = HISTORY_LOOKUP, offset = 0) }
                ?.firstOrNull { it.infohash == infohash && it.fileIdx.toInt() == fileIdx }
            if (seen != null) return Reclaimed(infohash, seen.collectionTitle, seen.season, seen.episode, seen.torrentName)
        }
        val current = c?.episodes?.firstOrNull { it.infohash == infohash && it.fileIdx.toInt() == fileIdx }
        return Reclaimed(infohash, c?.displayTitle, current?.season, current?.episode, name)
    }

    /**
     * Remove a dead release (frees the partial download), then search for another. A removal
     * the server refused is said, and nothing opens: the partial download is still there.
     */
    fun replace(deadSwarm: Boolean, onSearch: (String) -> Unit) {
        if (busyState.value != null) return
        busyState.value = BUSY_REPLACE
        viewModelScope.launch {
            if (deadSwarm) {
                val failed = try {
                    api().deleteTorrent(infohash)
                    null
                } catch (e: Exception) {
                    e.toUiError()
                }
                if (failed != null) {
                    busyState.value = null
                    say("Iris could not remove this release: ${failed.message}")
                    return@launch
                }
            }
            busyState.value = null
            val r = reclaimed()
            onSearch(retrySearchQuery(r.title, r.season, r.episode, r.name))
        }
    }

    /** "Grab and play" on a discovered episode, in the language its row shows. */
    fun grab(row: SideRow, onReady: (String, Int) -> Unit) {
        val target = row.grab ?: return
        val cid = collectionState.value?.id ?: return
        if (busyState.value != null) return
        busyState.value = row.key
        viewModelScope.launch {
            val res = try {
                api().grabCollectionEpisode(cid.toString(), target.season.toInt(), target.episode.toInt(), target.language)
            } catch (e: Exception) {
                say("Iris could not grab ${episodeCode(target.season, target.episode)}: ${e.toUiError().message}")
                null
            }
            busyState.value = null
            if (res != null) onReady(res.infohash, res.fileIdx.toInt())
        }
    }

    /** "Prepare it?" accepted: the follow grabs the next episode (in the series' dominant language, server-side). */
    fun prepareNext() {
        val next = contextState.value?.next ?: return
        val follow = next.followId ?: return
        if (busyState.value != null) return
        busyState.value = BUSY_PREPARE
        viewModelScope.launch {
            val code = episodeCode(next.season, next.episode)
            try {
                api().grabEpisode(follow.toString(), next.season.toInt(), next.episode.toInt())
                say("$code is being prepared.")
            } catch (e: Exception) {
                say("Iris could not prepare $code: ${e.toUiError().message}")
            }
            busyState.value = null
        }
    }

    private fun say(message: String) {
        noticeState.value = message
        viewModelScope.launch {
            delay(NOTICE_MS)
            noticeState.update { if (it == message) null else it }
        }
    }

    companion object {
        const val BUSY_REGRAB = "regrab"
        const val BUSY_REPLACE = "replace"
        const val BUSY_PREPARE = "prepare"
        private const val TORRENT_POLL_MS = 2_000L
        private const val PROBE_ATTEMPTS = 60
        private const val PROBE_RETRY_MS = 2_000L
        private const val PLAY_STATUS_POLL_MS = 1_500L
        private const val NOTICE_MS = 5_000L
        private const val HISTORY_LOOKUP = 200
    }
}
