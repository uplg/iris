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
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.api
import studio.kahn.iris.tv.data.TmdbMetadataCache
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.ui.state.BusyActions
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.isVideoPath
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.map
import studio.kahn.iris.tv.ui.format.prettySceneName
import studio.kahn.iris.tv.ui.components.Notice
import studio.kahn.iris.tv.ui.state.LiveRead
import studio.kahn.iris.tv.ui.state.FAST_MS
import studio.kahn.iris.tv.ui.state.SLOW_MS

/** A release's own page: what it is, what it does, its video files. */
@Immutable
data class ReleasePage(
    val title: String,
    val kind: String,
    val posterUrl: String?,
    val row: ReleaseRow,
    val files: List<ReleaseFile>,
)

@Immutable
data class ReleaseFile(val index: Int, val name: String, val facts: String, val resume: Boolean, val watched: Boolean)

@Immutable
data class DetailUiState(
    val page: Loadable<ReleasePage> = Loadable.Loading,
    val busy: Set<String> = emptySet(),
    val notice: Notice? = null,
    /** The release was deleted: the page has nothing left to show. */
    val gone: Boolean = false,
)

private fun FileProgressEntry.toWatch() = WatchState(
    pct = if (completed) 100.0 else durationSeconds?.takeIf { it > 0 }?.let { minOf(100.0, positionSeconds / it * 100) },
    done = completed,
)

/**
 * One release and its video files (reached from a multi-file release in the library or a
 * search): play or resume each file, pause or resume the release, delete it. Read again
 * every few seconds while it downloads, only while the screen is started.
 */
class DetailViewModel(private val container: AppContainer, private val infohash: String) : ViewModel() {
    private val torrent = LiveRead({ t: TorrentView? -> if (t != null && moving(t)) FAST_MS else SLOW_MS }) {
        container.api().getTorrent(infohash)
    }
    private val progress = MutableStateFlow<List<FileProgressEntry>>(emptyList())
    private val meta = MutableStateFlow<MediaMetadata?>(null)
    private val gone = MutableStateFlow(false)
    private val actions = BusyActions(viewModelScope)
    private var metaAsking = false

    val state: StateFlow<DetailUiState> = combine(torrent.state, progress, meta, actions.state, gone) { t, p, m, a, g ->
        DetailUiState(page = t.map { page(it, p, m) }, busy = a.busy, notice = a.notice, gone = g)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), DetailUiState())

    suspend fun pollWhileStarted() = coroutineScope {
        launch { bestEffort { container.api().torrentProgress(infohash) }?.let { progress.value = it } }
        launch {
            torrent.state.collect { s ->
                val t = s.valueOrNull ?: return@collect
                val id = t.tmdbId
                // Until it is in: a failed or cut read is asked again on the next change or start.
                if (meta.value == null && !metaAsking && id != null) {
                    metaAsking = true
                    launch {
                        try {
                            meta.value = TmdbMetadataCache.get(container.api(), id, t.kind?.value)
                        } finally {
                            metaAsking = false
                        }
                    }
                }
            }
        }
        torrent.poll()
    }

    fun retry() = torrent.poke()

    fun pause(r: ReleaseRow) = act("pause:${r.infohash}", "Paused. The files stay on disk.") {
        container.api().pauseTorrent(r.infohash)
        torrent.refresh()
    }

    fun resume(r: ReleaseRow) = act("resume:${r.infohash}", "Back in the swarm.") {
        container.api().resumeTorrent(r.infohash)
        torrent.refresh()
    }

    fun delete(r: ReleaseRow) = act("delete:${r.infohash}", "Deleted ${r.release}.") {
        container.api().deleteTorrent(r.infohash)
        gone.value = true
    }

    private fun act(key: String, done: String, block: suspend CoroutineScope.() -> Unit) {
        actions.runSaying(key) {
            block()
            done
        }
    }
}

private fun page(t: TorrentView, progress: List<FileProgressEntry>, m: MediaMetadata?): ReleasePage {
    val byIdx = progress.associateBy { it.fileIdx.toInt() }
    val title = t.name?.let(::prettySceneName) ?: t.infohash
    return ReleasePage(
        title = title,
        kind = if (t.kind == studio.kahn.iris.tv.data.MediaKind.tv) "Series" else "Movie",
        posterUrl = tmdbPosterUrl(m?.posterPath, "w342"),
        row = releaseRow(t, title, m?.posterPath, null, Instant.now()).copy(playIdx = null),
        files = t.files.filter { isVideoPath(it.path) }.sortedBy { it.path }.map { f ->
            val w = byIdx[f.index]?.toWatch() ?: WatchState(null, false)
            ReleaseFile(f.index, f.path.substringAfterLast('/'), fileFacts(f.sizeBytes, w), w.resumable, w.done)
        },
    )
}
