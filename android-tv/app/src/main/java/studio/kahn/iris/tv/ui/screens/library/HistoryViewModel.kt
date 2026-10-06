package studio.kahn.iris.tv.ui.screens.library

import androidx.compose.runtime.Immutable
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import java.time.Instant
import java.time.ZoneId
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.HistoryItem
import studio.kahn.iris.tv.data.ResolveBody
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.map
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.components.Notice

/** What OK does on a history line. */
enum class LineAction { Play, Restore, OpenTitle, None }

@Immutable
data class HistoryLine(
    val key: String,
    val label: String,
    /** The line named with its title, for an action's words. */
    val words: String,
    val facts: String,
    val share: Float,
    val deleted: Boolean,
    val action: LineAction,
    val infohash: String,
    val fileIdx: Int,
)

@Immutable
data class HistoryGroupUi(
    val key: String,
    val collectionId: String?,
    val title: String,
    val posterUrl: String?,
    val ghost: Boolean,
    val solo: Boolean,
    val lines: List<HistoryLine>,
)

@Immutable
data class HistoryUiState(
    val groups: Loadable<List<HistoryGroupUi>> = Loadable.Loading,
    val busy: Set<String> = emptySet(),
    val notice: Notice? = null,
)

/** The server's most: the list stays light at any length (a lazy list). */
private const val LIMIT = 200

/**
 * My watch history (web `/history`): everything watched, finished or not, grouped by title,
 * including what was since reclaimed from disk (a ghost, never dropped). A gone release is
 * downloaded again in one press and plays from where it stopped.
 */
class HistoryViewModel(private val container: AppContainer) : ViewModel() {
    private val history = LiveRead({ _: List<HistoryItem>? -> 5 * 60_000L }) { container.api().history(limit = LIMIT, offset = 0) }
    private val controls = MutableStateFlow(HistoryUiState())
    private val play = MutableStateFlow<Pair<String, Int>?>(null)
    val playEvents: StateFlow<Pair<String, Int>?> = play

    val state: StateFlow<HistoryUiState> = combine(history.state, controls) { h, c ->
        c.copy(groups = h.map { items -> historyUi(items, Instant.now()) })
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), HistoryUiState())

    suspend fun pollWhileStarted() = history.poll()

    fun retry() = history.poke()

    fun consumePlay() {
        play.value = null
    }

    fun restore(line: HistoryLine) {
        val item = history.value?.firstOrNull { historyKey(it) == line.key } ?: return
        val provider = item.sourceProvider ?: return
        val external = item.sourceExternalId ?: return
        val key = "restore:${line.key}"
        if (key in controls.value.busy) return
        controls.update { it.copy(busy = it.busy + key, notice = null) }
        viewModelScope.launch {
            val notice = try {
                container.api().ingest(ResolveBody(providerId = provider, externalId = external, tmdbId = item.tmdbId, allowDuplicate = true))
                history.refresh()
                play.value = item.infohash to item.fileIdx.toInt()
                null
            } catch (e: Exception) {
                Notice(e.toUiError().message, failed = true)
            }
            controls.update { it.copy(busy = it.busy - key, notice = notice) }
        }
    }
}

fun historyUi(items: List<HistoryItem>, now: Instant, zone: ZoneId = ZoneId.systemDefault()): List<HistoryGroupUi> = groupHistory(items).map { g ->
    HistoryGroupUi(
        key = g.key,
        collectionId = g.collectionId,
        title = g.title,
        posterUrl = tmdbPosterUrl(g.posterPath, "w185"),
        ghost = g.ghost,
        solo = g.solo,
        lines = g.items.map { it ->
            val label = historyLabel(g, it)
            HistoryLine(
                key = historyKey(it),
                label = label,
                words = if (g.solo) label else "${g.title}, $label",
                facts = historyFacts(it, now, zone),
                share = watchedShare(it.positionSeconds, it.durationSeconds, it.completed),
                deleted = it.deleted,
                action = when {
                    !it.deleted -> LineAction.Play
                    canRestore(it) -> LineAction.Restore
                    g.solo && g.collectionId != null -> LineAction.OpenTitle
                    else -> LineAction.None
                },
                infohash = it.infohash,
                fileIdx = it.fileIdx.toInt(),
            )
        },
    )
}
