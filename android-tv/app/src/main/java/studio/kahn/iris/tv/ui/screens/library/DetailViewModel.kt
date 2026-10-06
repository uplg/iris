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
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.isVideoPath
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.map
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.format.prettySceneName

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
    private val controls = MutableStateFlow(DetailUiState())
    private var metaAsked = false

    val state: StateFlow<DetailUiState> = combine(torrent.state, progress, meta, controls) { t, p, m, c ->
        c.copy(page = t.map { page(it, p, m) })
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), DetailUiState())

    suspend fun pollWhileStarted() = coroutineScope {
        launch { progress.value = runCatching { container.api().torrentProgress(infohash) }.getOrDefault(progress.value) }
        launch {
            torrent.state.collect { s ->
                val t = s.valueOrNull ?: return@collect
                val id = t.tmdbId
                if (!metaAsked && id != null) {
                    metaAsked = true
                    launch { meta.value = runCatching { container.api().tmdbMetadata(id, t.kind?.value) }.getOrNull() }
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
        controls.update { it.copy(gone = true) }
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
