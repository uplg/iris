package studio.kahn.iris.tv.ui.screens.live

import android.content.SharedPreferences
import androidx.compose.runtime.Immutable
import androidx.core.content.edit
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.transformLatest
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveCountry
import studio.kahn.iris.tv.data.LiveNowNext
import studio.kahn.iris.tv.data.LiveSearchResult
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.STOP_TIMEOUT_MS
import studio.kahn.iris.tv.ui.state.load
import studio.kahn.iris.tv.ui.state.polling

/** The guide of a country, and when it was read (the "now" its progress bars use). */
@Immutable
data class LiveGuide(val entries: Map<String, LiveNowNext> = emptyMap(), val readAtMs: Long = 0L)

/** Search results grouped by country, the web's way. */
@Immutable
data class LiveResults(val query: String, val byCountry: List<Pair<String, List<LiveSearchResult>>>)

/**
 * Live TV, the channel list (web `/live`): the picked country (remembered
 * across restarts), its channels, its guide refreshed every minute while the
 * screen is started, and a channel search across every country (server-side,
 * after a 300 ms pause in typing).
 */
@OptIn(ExperimentalCoroutinesApi::class)
class LiveTvViewModel(
    private val container: AppContainer,
    private val prefs: SharedPreferences,
) : ViewModel() {
    private val serverUrl = MutableStateFlow<String?>(null)
    val baseUrl: StateFlow<String?> = serverUrl.asStateFlow()

    private val countriesState = MutableStateFlow<List<LiveCountry>>(emptyList())
    val countries: StateFlow<List<LiveCountry>> = countriesState.asStateFlow()

    private val countryState = MutableStateFlow(prefs.getString(PREF_COUNTRY, null))
    val country: StateFlow<String?> = countryState.asStateFlow()

    private val reload = MutableStateFlow(0)

    val channels: StateFlow<Loadable<List<LiveChannel>>> = countryState.filterNotNull()
        .transformLatest { c ->
            emit(Loadable.Loading)
            reload.collect {
                emit(load(Loadable.Loading) { channelsOf(c) })
            }
        }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), Loadable.Loading)

    val guide: StateFlow<LiveGuide> = countryState.filterNotNull()
        .transformLatest { c ->
            emit(LiveGuide())
            polling(EPG_REFRESH_MS, from = { Loadable.Loading }) { api().liveTvEpgNow(c) }
                .collect { state ->
                    state.valueOrNull?.let { res ->
                        emit(LiveGuide(res.propertyEntries.associateBy { it.channelId }, System.currentTimeMillis()))
                    }
                }
        }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), LiveGuide())

    private val queryState = MutableStateFlow("")
    val query: StateFlow<String> = queryState.asStateFlow()

    /** Null while the query is under two characters (the grid shows instead). */
    val results: StateFlow<Loadable<LiveResults>?> = queryState
        .map { it.trim() }
        .transformLatest { q ->
            if (q.length < 2) {
                emit(null)
                return@transformLatest
            }
            delay(SEARCH_DEBOUNCE_MS)
            emit(Loadable.Loading)
            emit(
                load(Loadable.Loading) {
                    val base = serverUrl.value.orEmpty()
                    val hits = api().liveTvSearch(q).results.map {
                        it.copy(logoUrl = absolutize(base, it.logoUrl))
                    }
                    LiveResults(q, hits.groupBy { it.country }.toList())
                },
            )
        }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), null)

    init {
        viewModelScope.launch {
            serverUrl.value = container.sessionStore.serverUrl.first()
            runCatching { api().liveTvCountries() }
                .onSuccess { res ->
                    countriesState.value = res.countries
                    if (countryState.value == null) countryState.value = res.defaultCountry
                }
                .onFailure { if (countryState.value == null) countryState.value = "fr" }
        }
    }

    private suspend fun api(): IrisApi {
        val url = serverUrl.value ?: container.sessionStore.serverUrl.first()?.also { serverUrl.value = it }
            ?: throw IllegalStateException("This TV is signed out. Pair it again from Settings.")
        return container.apiFor(url)
    }

    // Logo URLs are server-relative: Coil needs them absolute.
    private suspend fun channelsOf(country: String): List<LiveChannel> {
        val api = api()
        val base = serverUrl.value.orEmpty()
        return api.liveTvChannels(country).channels.map { it.copy(logoUrl = absolutize(base, it.logoUrl)) }
    }

    fun pickCountry(code: String) {
        prefs.edit { putString(PREF_COUNTRY, code) }
        countryState.value = code
    }

    fun onQueryChange(q: String) {
        queryState.value = q
    }

    fun retry() {
        reload.value++
    }

    /** The country's display name, for the search results' headings. */
    fun countryName(code: String): String =
        countriesState.value.firstOrNull { it.code == code }?.name ?: code.uppercase()

    companion object {
        const val PREFS = "livetv"
        private const val PREF_COUNTRY = "country"
        private const val EPG_REFRESH_MS = 60_000L
        private const val SEARCH_DEBOUNCE_MS = 300L
    }
}
