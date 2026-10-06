package studio.kahn.iris.tv.ui.screens.live

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.transformLatest
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.load

/**
 * A channel watched (LiveTvWatchScreen): the country's channels to zap through and its guide,
 * read like the channel list reads them ([LiveReads]) and only while the screen is started;
 * the channel zapped to survives the activity being recreated.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class LiveWatchViewModel(
    container: AppContainer,
    val country: String,
    initialChannelId: String,
    private val saved: SavedStateHandle,
) : ViewModel() {
    private val reads = LiveReads(container)
    val baseUrl: StateFlow<String?> = reads.baseUrl

    val channelId: StateFlow<String> = saved.getStateFlow(KEY_CHANNEL, initialChannelId)

    private val reload = MutableStateFlow(0)
    val channels: StateFlow<Loadable<List<LiveChannel>>> = reload
        .transformLatest { emit(load(Loadable.Loading) { reads.channelsOf(country) }) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), Loadable.Loading)

    val guide: StateFlow<LiveGuide> = reads.guide(country, EPG_REFRESH_MS)
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), LiveGuide())

    init {
        viewModelScope.launch { bestEffort { reads.api() } }
    }

    fun watch(id: String) {
        saved[KEY_CHANNEL] = id
    }

    /** The channel [delta] places away in the list, or null while the list is not there. */
    fun channelAt(delta: Int): String? {
        val list = channels.value.valueOrNull?.takeIf { it.isNotEmpty() } ?: run {
            retryChannels()
            return null
        }
        val idx = list.indexOfFirst { it.id == channelId.value }.coerceAtLeast(0)
        return list[(idx + delta + list.size) % list.size].id
    }

    fun retryChannels() {
        if (channels.value !is Loadable.Ready) reload.value++
    }

    /** The feed of [id] failed: the server demotes it and elects the next one. */
    suspend fun reportFailure(id: String) {
        bestEffort { reads.api().liveTvPlaybackError(country, id) }
    }

    private companion object {
        const val KEY_CHANNEL = "live_channel"
        const val EPG_REFRESH_MS = 30_000L
    }
}
