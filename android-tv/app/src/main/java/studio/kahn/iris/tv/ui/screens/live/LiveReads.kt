package studio.kahn.iris.tv.ui.screens.live

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.mapNotNull
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.polling

/**
 * The live TV reads both live screens make (the channel list, a channel watched): a country's
 * channels, logos made absolute, and its guide polled while collected.
 */
class LiveReads(private val container: AppContainer) {
    private val serverUrl = MutableStateFlow<String?>(null)
    val baseUrl: StateFlow<String?> = serverUrl.asStateFlow()

    suspend fun api(): IrisApi {
        val url = serverUrl.value ?: container.sessionStore.serverUrl.first()?.also { serverUrl.value = it }
            ?: throw IllegalStateException("This TV is signed out. Pair it again from Settings.")
        return container.apiFor(url)
    }

    // Logo URLs are server-relative: Coil needs them absolute.
    suspend fun channelsOf(country: String): List<LiveChannel> {
        val api = api()
        val base = serverUrl.value.orEmpty()
        return api.liveTvChannels(country).channels.map { it.copy(logoUrl = absolutize(base, it.logoUrl)) }
    }

    /** The country's guide every [intervalMs] while collected; a failed read keeps the last one. */
    fun guide(country: String, intervalMs: Long): Flow<LiveGuide> =
        polling(intervalMs, from = { Loadable.Loading }) { api().liveTvEpgNow(country) }
            .mapNotNull { state ->
                state.valueOrNull?.let { res -> LiveGuide(res.propertyEntries.associateBy { it.channelId }, System.currentTimeMillis()) }
            }
}
