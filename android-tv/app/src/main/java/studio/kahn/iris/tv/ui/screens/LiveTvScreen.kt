package studio.kahn.iris.tv.ui.screens

import studio.kahn.iris.tv.ui.format.clockTime
import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.time.OffsetDateTime
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.screens.live.LiveTvContent
import studio.kahn.iris.tv.ui.screens.live.LiveTvUi
import studio.kahn.iris.tv.ui.screens.live.LiveTvViewModel
import studio.kahn.iris.tv.ui.state.irisViewModel

/**
 * Live TV, the channel list (web `/live`). Opening a channel plays it in
 * [LiveTvWatchScreen]. A top-level section: the shell draws the header, the
 * top safe margin and Back to the header.
 */
@Composable
fun LiveTvScreen(
    container: AppContainer,
    onOpenChannel: (country: String, channelId: String) -> Unit,
) {
    val context = LocalContext.current
    val vm = irisViewModel(container) { c, _ ->
        LiveTvViewModel(c, context.applicationContext.getSharedPreferences(LiveTvViewModel.PREFS, Context.MODE_PRIVATE))
    }
    val countries by vm.countries.collectAsStateWithLifecycle()
    val country by vm.country.collectAsStateWithLifecycle()
    val channels by vm.channels.collectAsStateWithLifecycle()
    val guide by vm.guide.collectAsStateWithLifecycle()
    val query by vm.query.collectAsStateWithLifecycle()
    val results by vm.results.collectAsStateWithLifecycle()
    val usual by vm.usual.collectAsStateWithLifecycle()
    val clock: (OffsetDateTime) -> String = ::clockTime
    LiveTvContent(
        ui = LiveTvUi(countries, country, channels, guide, query, results, usual),
        clock = clock,
        countryName = vm::countryName,
        onQueryChange = vm::onQueryChange,
        onPickCountry = vm::pickCountry,
        onOpen = onOpenChannel,
        onRetry = vm::retry,
    )
}
