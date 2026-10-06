package studio.kahn.iris.tv.ui.screens.home

import android.os.SystemClock
import androidx.compose.runtime.Immutable
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CatalogCard
import studio.kahn.iris.tv.data.DismissRequest
import studio.kahn.iris.tv.data.ForYou
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.MoodBoard
import studio.kahn.iris.tv.data.MoodResults
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.map
import studio.kahn.iris.tv.ui.state.toUiError
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.components.Notice

/** One mood's titles, in words. */
@Immutable
data class MoodResultsModel(val id: String, val label: String, val count: String?, val cards: Loadable<List<CardModel>>)

@Immutable
data class DiscoverUiState(
    val kind: MediaKind = MediaKind.movie,
    val board: Loadable<List<MoodModel>> = Loadable.Loading,
    /** Non-null while a mood is open (Back returns to the board). */
    val mood: MoodResultsModel? = null,
    val forYou: Loadable<List<ShelfModel>> = Loadable.Loading,
    val busy: String? = null,
    val notice: Notice? = null,
)

@Immutable
internal data class DiscoverData(
    val kind: MediaKind = MediaKind.movie,
    val mood: String? = null,
    val boards: Map<MediaKind, Loadable<MoodBoard>> = emptyMap(),
    val results: Map<Pair<String, MediaKind>, Loadable<MoodResults>> = emptyMap(),
    val forYou: Loadable<ForYou> = Loadable.Loading,
    val busy: String? = null,
    val notice: Notice? = null,
)

const val MOOD_PREFIX = "mood:"

internal fun discoverUi(d: DiscoverData): DiscoverUiState {
    val board = d.boards[d.kind] ?: Loadable.Loading
    val mood = d.mood?.let { id ->
        val results = d.results[id to d.kind] ?: Loadable.Loading
        val noun = if (d.kind == MediaKind.tv) "series" else "movie"
        val nouns = if (d.kind == MediaKind.tv) "series" else "movies"
        MoodResultsModel(
            id = id,
            label = board.valueOrNull?.moods?.firstOrNull { it.id == id }?.label ?: "This mood",
            count = results.valueOrNull?.let { plural(it.items.size.toLong(), noun, nouns) },
            cards = results.map { r -> r.items.map { catalogCard(it, MOOD_PREFIX) } },
        )
    }
    return DiscoverUiState(
        kind = d.kind,
        board = board.map { b -> b.moods.map(::moodModel) },
        mood = mood,
        forYou = d.forYou.map { shelves(it.shelves) },
        busy = d.busy,
        notice = d.notice,
    )
}

sealed interface DiscoverEvent {
    data class Search(val query: String) : DiscoverEvent
}

/**
 * Discover (web `routes/discover`): tonight's moods for movies or series (a board of tiles,
 * then one mood's titles), then every suggestion shelf. The kind and the open mood live in
 * the saved state, so Back from a title lands on the same mood.
 */
class DiscoverViewModel(
    private val container: AppContainer,
    private val saved: SavedStateHandle,
    private val now: () -> Long = SystemClock::elapsedRealtime,
) : ViewModel() {
    private val data = MutableStateFlow(
        DiscoverData(
            kind = if (saved.get<String>(KEY_KIND) == MediaKind.tv.value) MediaKind.tv else MediaKind.movie,
            mood = saved.get<String>(KEY_MOOD),
        ),
    )
    val state: StateFlow<DiscoverUiState> = data.map(::discoverUi)
        .stateIn(viewModelScope, SharingStarted.Eagerly, discoverUi(data.value))

    private val eventChannel = Channel<DiscoverEvent>(Channel.BUFFERED)
    val events: Flow<DiscoverEvent> = eventChannel.receiveAsFlow()
    private var forYouReadAt: Long? = null

    /** Run by the screen each time it starts: what it shows, read again (suggestions at most once a minute). */
    suspend fun refreshOnStart() {
        val d = data.value
        viewModelScope.launch { readBoard(d.kind) }
        d.mood?.let { id -> viewModelScope.launch { readResults(id, d.kind) } }
        val at = forYouReadAt
        if (at == null || now() - at >= FOR_YOU_STALE_MS) readForYou()
    }

    fun retry() {
        val d = data.value
        viewModelScope.launch { readBoard(d.kind) }
        d.mood?.let { id -> viewModelScope.launch { readResults(id, d.kind) } }
        viewModelScope.launch { readForYou() }
    }

    fun setKind(kind: MediaKind) {
        if (kind == data.value.kind) return
        saved[KEY_KIND] = kind.value
        data.update { it.copy(kind = kind, notice = null) }
        if (data.value.boards[kind]?.valueOrNull == null) viewModelScope.launch { readBoard(kind) }
        data.value.mood?.let { id -> viewModelScope.launch { readResults(id, kind) } }
    }

    fun openMood(id: String) {
        saved[KEY_MOOD] = id
        data.update { it.copy(mood = id, notice = null) }
        viewModelScope.launch { readResults(id, data.value.kind) }
    }

    fun closeMood() {
        saved.remove<String>(KEY_MOOD)
        data.update { it.copy(mood = null) }
    }

    fun onCardAction(key: String, action: CardAction) {
        val card = findCard(key) ?: return
        when (action) {
            CardAction.FindReleases -> eventChannel.trySend(DiscoverEvent.Search(card.title))
            CardAction.NotInterested -> dismiss(key, card)
            else -> Unit
        }
    }

    private fun findCard(key: String): CatalogCard? {
        val d = data.value
        if (key.startsWith(MOOD_PREFIX)) {
            val results = d.mood?.let { d.results[it to d.kind]?.valueOrNull } ?: return null
            return results.items.firstOrNull { MOOD_PREFIX + it.catalogId == key }
        }
        return d.forYou.valueOrNull?.shelves?.firstNotNullOfOrNull { shelf ->
            shelf.items.firstOrNull { "fy:${shelf.key}:${it.catalogId}" == key }
        }
    }

    /** Hidden from every suggestion surface, on the server's word. */
    private fun dismiss(key: String, card: CatalogCard) {
        if (data.value.busy != null) return
        data.update { it.copy(busy = key, notice = null) }
        viewModelScope.launch {
            val notice = try {
                container.api().dismissForYou(DismissRequest(card.catalogId))
                readForYou()
                data.value.mood?.let { readResults(it, data.value.kind) }
                Notice("${card.title} hidden from your suggestions")
            } catch (e: Exception) {
                Notice(e.toUiError().message, StatusTone.Down)
            }
            data.update { it.copy(busy = null, notice = notice) }
        }
    }

    private suspend fun readBoard(kind: MediaKind) {
        val next = load(data.value.boards[kind] ?: Loadable.Loading) { container.api().moodBoard(kind.value) }
        data.update { it.copy(boards = it.boards + (kind to next)) }
    }

    private suspend fun readResults(id: String, kind: MediaKind) {
        val key = id to kind
        val next = load(data.value.results[key] ?: Loadable.Loading) { container.api().moodResults(id, kind.value) }
        data.update { it.copy(results = it.results + (key to next)) }
    }

    private suspend fun readForYou() {
        val next = load(data.value.forYou) { container.api().forYouPage() }
        if (next is Loadable.Ready) forYouReadAt = now()
        data.update { it.copy(forYou = next) }
    }

    private companion object {
        const val KEY_KIND = "kind"
        const val KEY_MOOD = "mood"
        const val FOR_YOU_STALE_MS = 60_000L
    }
}
