package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.runtime.Immutable
import androidx.compose.ui.text.AnnotatedString
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CreateFollowRequest
import studio.kahn.iris.tv.data.GrabTarget
import studio.kahn.iris.tv.data.ReleaseFile
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.TorrentDetails
import studio.kahn.iris.tv.data.TorrentPreview
import studio.kahn.iris.tv.data.asReleaseFile
import studio.kahn.iris.tv.data.autoFile
import studio.kahn.iris.tv.data.playWords
import studio.kahn.iris.tv.data.playableFiles
import studio.kahn.iris.tv.data.ResolveBody
import studio.kahn.iris.tv.data.sceneMark
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.format.formatRelative
import studio.kahn.iris.tv.ui.format.formatSize
import studio.kahn.iris.tv.ui.format.kindWord
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.format.prettySceneName

/** Following the series this release belongs to. */
@Immutable
sealed interface FollowState {
    /** A movie, or not known yet. */
    data object Hidden : FollowState
    data class Can(val busy: Boolean = false, val error: UiError? = null) : FollowState
    data object Following : FollowState
}

@Immutable
data class ReleaseUiState(
    val providerId: String,
    val externalId: String,
    /** What the search knew about it (seeders, poster, TMDB title, library state). */
    val hit: SearchResult? = null,
    val routeTmdbId: Long? = null,
    val routeKind: String? = null,
    /** The `.torrent`: files, size, whether it streams. A full leech slot fails it, named. */
    val preview: Loadable<TorrentPreview> = Loadable.Loading,
    /** The tracker's page, best effort: some trackers have none. */
    val details: TorrentDetails? = null,
    val notes: AnnotatedString? = null,
    val pickedFile: Int? = null,
    val follow: FollowState = FollowState.Hidden,
) {
    val sheet: ReleaseSheet get() = releaseSheet(this)
}

/** The release said for the page, from what the search, the torrent and the tracker know. */
@Immutable
data class ReleaseSheet(
    val title: String,
    val heading: String,
    val kindLine: String?,
    val name: String,
    val posterUrl: String?,
    val chips: List<String>,
    val freeleech: Boolean,
    val tmdbId: Long?,
    val isTv: Boolean,
    val dead: Boolean,
    val archive: Boolean,
    val owned: OwnedFile?,
    val videos: List<ReleaseFile>,
    val chosenFile: Int?,
    val playLabel: String,
    val facts: List<Pair<String, String>>,
) {
    /** Why the grab cannot start, in words; null = it can. */
    val blocked: String?
        get() = when {
            dead -> "$DEAD: this release cannot be downloaded. Try another one."
            archive -> ARCHIVE_WORDS
            else -> null
        }
}

fun releaseSheet(s: ReleaseUiState): ReleaseSheet {
    val p = s.preview.valueOrNull
    val d = s.details
    val hit = s.hit
    val name = p?.name ?: d?.title ?: hit?.title.orEmpty()
    val mark = sceneMark(name)
    val title = hit?.titleMatch?.title ?: if (name.isNotEmpty()) prettySceneName(name) else "Release"
    val kind = hit?.titleMatch?.kind?.value ?: hit?.kind?.value ?: s.routeKind ?: if (mark != null) "tv" else null
    val year = hit?.titleMatch?.year ?: hit?.year
    val part = partWords(hit?.parsedSeason ?: mark?.season, hit?.parsedEpisode ?: mark?.episode, name)
    val heading = listOfNotNull(title, part ?: if (kind == "movie") year?.toString() else null).joinToString(" · ")
    val poster = hit?.posterUrl ?: tmdbPosterUrl(hit?.titleMatch?.posterPath)
    val seeders = d?.seeders ?: hit?.seeders
    val freeleech = d?.freeleech ?: hit?.freeleech ?: false
    val chips = listOfNotNull(
        if (freeleech) "Freeleech" else null,
        if (d?.exclusive == true) "Exclusive" else null,
        s.providerId,
        languageLabel(hit?.languageTag),
    ) + (d?.tags ?: hit?.tags.orEmpty()).take(6)
    val files = p?.files?.map { it.asReleaseFile() }.orEmpty()
    val chosen = s.pickedFile ?: p?.let { autoFile(files) }
    val leechers = d?.leechers ?: hit?.leechers
    val swarm = listOfNotNull(
        seedersWords(seeders),
        leechers?.let { "$it leechers" },
        d?.timesCompleted?.let { "${String.format(java.util.Locale.ENGLISH, "%,d", it)} downloads" },
    ).joinToString(" · ")
    val uploadedAt = d?.uploadedAt ?: hit?.uploadedAt
    val uploader = d?.uploader ?: hit?.uploader
    val uploaded = listOfNotNull(
        uploadedAt?.let { formatRelative(it) } ?: d?.age?.let { "$it ago" },
        uploader?.let { "by $it" },
    ).joinToString(" ")
    val filesFact = p?.let { "${plural(it.files.size, "file")} · ${formatSize(it.totalSizeBytes)}" }
    val facts = listOfNotNull(
        swarm.ifEmpty { null }?.let { "Swarm" to it },
        uploaded.ifEmpty { null }?.let { "Uploaded" to it },
        videoWords(d?.mediaInfo)?.let { "Video" to it },
        audioWords(d?.mediaInfo)?.let { "Audio" to it },
        subtitleWords(d?.mediaInfo)?.let { "Subtitles" to it },
        filesFact?.let { "Files" to it },
    )
    return ReleaseSheet(
        title = title,
        heading = heading,
        kindLine = listOfNotNull(kindWord(kind), year?.toString()).joinToString(" · ").ifEmpty { null },
        name = name,
        posterUrl = poster,
        chips = chips.distinct(),
        freeleech = freeleech,
        tmdbId = hit?.titleMatch?.tmdbId ?: hit?.tmdbId ?: s.routeTmdbId,
        isTv = kind == "tv",
        dead = isDead(seeders),
        archive = p != null && !p.streamable,
        owned = hit?.let(::ownedFile),
        videos = playableFiles(files),
        chosenFile = chosen,
        playLabel = if (p != null) playWords(files, chosen) else "Download and play",
        facts = facts,
    )
}

/**
 * A release before grabbing it (web `release/ReleaseScreen.svelte`): the
 * search's hit, the torrent's preview and the tracker's details read side
 * by side; the file to play; following the series; the grab, with its
 * refusals said before anything downloads.
 */
class ReleaseViewModel(
    private val container: AppContainer,
    providerId: String,
    externalId: String,
    tmdbId: Long?,
    kind: String?,
) : ViewModel() {
    private val mutable = MutableStateFlow(
        ReleaseUiState(
            providerId = providerId,
            externalId = externalId,
            hit = SearchMemory.release(providerId, externalId),
            routeTmdbId = tmdbId,
            routeKind = kind,
        ),
    )
    val state: StateFlow<ReleaseUiState> = mutable.asStateFlow()
    val grabber = Grabber(container, viewModelScope)
    private var followAsked = false

    init {
        loadPreview()
        viewModelScope.launch {
            val details = runCatching { container.api().torrentDetails(providerId, externalId) }.getOrNull()
            val notes = details?.description?.takeIf { it.isNotBlank() }?.let { source ->
                withContext(Dispatchers.Default) { releaseNotes(source, details.descriptionFormat) }
            }
            mutable.update { it.copy(details = details, notes = notes) }
            loadFollow()
        }
    }

    fun loadPreview() {
        viewModelScope.launch {
            mutable.update { it.copy(preview = Loadable.Loading) }
            val next = load(Loadable.Loading) { container.api().previewTorrent(body()) }
            mutable.update { it.copy(preview = next) }
            loadFollow()
        }
    }

    fun pickFile(index: Int) = mutable.update { it.copy(pickedFile = index) }

    fun grab() {
        val s = mutable.value
        val sheet = s.sheet
        val preview = s.preview.valueOrNull ?: return
        if (sheet.blocked != null) return
        grabber.run(KEY, GrabTarget(s.providerId, s.externalId, sheet.tmdbId, preview, sheet.chosenFile))
    }

    fun playOwned() {
        mutable.value.sheet.owned?.let(grabber::playOwned)
    }

    fun follow() {
        val s = mutable.value
        if (s.follow !is FollowState.Can || s.follow.busy) return
        val sheet = s.sheet
        mutable.update { it.copy(follow = FollowState.Can(busy = true)) }
        viewModelScope.launch {
            val next = try {
                container.api().addFollow(CreateFollowRequest(name = sheet.title, tmdbId = sheet.tmdbId))
                FollowState.Following
            } catch (e: Exception) {
                FollowState.Can(error = e.toUiError())
            }
            mutable.update { it.copy(follow = next) }
        }
    }

    /** Once the kind is known: is this series already followed? */
    private suspend fun loadFollow() {
        val sheet = mutable.value.sheet
        if (followAsked || !sheet.isTv) return
        followAsked = true
        val follows = runCatching { container.api().listFollows() }.getOrNull() ?: return
        val following = follows.any { f ->
            (sheet.tmdbId != null && f.tmdbId == sheet.tmdbId) || f.name.equals(sheet.title, ignoreCase = true)
        }
        mutable.update { it.copy(follow = if (following) FollowState.Following else FollowState.Can()) }
    }

    private fun body() = ResolveBody(
        externalId = mutable.value.externalId,
        providerId = mutable.value.providerId,
        tmdbId = mutable.value.sheet.tmdbId,
    )

    companion object {
        const val KEY = "release"
    }
}
