package studio.kahn.iris.tv.ui.nav

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.navigation.NavBackStackEntry
import androidx.navigation.NavDestination
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavGraphBuilder
import androidx.navigation.NavHostController
import androidx.navigation.NavOptionsBuilder
import androidx.navigation.compose.composable
import studio.kahn.iris.tv.ui.components.TopTab
import studio.kahn.iris.tv.ui.components.TvHeader
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace

/** The route a header tab opens. */
fun TopTab.route(): Any = when (this) {
    TopTab.Home -> Routes.Home
    TopTab.Search -> Routes.Search()
    TopTab.Discover -> Routes.Discover
    TopTab.Library -> Routes.Library
    TopTab.LiveTv -> Routes.LiveTv
}

/** The header tab a destination belongs to; null off the five sections (a title, the player, Settings). */
fun NavDestination.topTab(): TopTab? = when {
    hasRoute<Routes.Home>() -> TopTab.Home
    hasRoute<Routes.Search>() -> TopTab.Search
    hasRoute<Routes.Discover>() -> TopTab.Discover
    hasRoute<Routes.Library>() -> TopTab.Library
    hasRoute<Routes.LiveTv>() -> TopTab.LiveTv
    else -> null
}

/**
 * Opens a section the way a tab bar does: one entry per section above Home
 * (Home → Library → Search leaves Home, Search), each section keeping where
 * it was (its own back stack is saved and restored). Home pops back to itself.
 */
fun NavHostController.openTab(tab: TopTab) {
    if (tab == TopTab.Home) {
        if (!popBackStack(Routes.Home, inclusive = false)) {
            navigate(Routes.Home) { popUpTo(graph.id) { inclusive = true } }
        }
        return
    }
    navigate(tab.route()) { asTab() }
}

private fun NavOptionsBuilder.asTab() {
    popUpTo(Routes.Home) { saveState = true }
    launchSingleTop = true
    restoreState = true
}

/** What every [section] shares: the account's words and where the header leads. */
@Stable
class ShellHost(
    val accountName: State<String>,
    val onSelect: (TopTab) -> Unit,
    val onAccount: () -> Unit,
)

/**
 * A top-level destination ([tab] in the header): `composable<T>` with its
 * content inside the [TopLevelShell].
 */
inline fun <reified T : Any> NavGraphBuilder.section(
    tab: TopTab,
    host: ShellHost,
    noinline content: @Composable (NavBackStackEntry) -> Unit,
) {
    composable<T> { entry ->
        TopLevelShell(
            tab = tab,
            accountName = host.accountName.value,
            onSelect = host.onSelect,
            onAccount = host.onAccount,
        ) { content(entry) }
    }
}

/** What Back does on a top-level section. */
enum class ShellBack {
    /** Focus is in the section's content: up to its tab in the header. */
    ToHeader,
    /** The header has focus, on a section other than Home. */
    ToHome,
    /** The header has focus on Home: the app leaves, as the platform does. */
    Leave,
}

fun shellBack(tab: TopTab, headerFocused: Boolean): ShellBack = when {
    !headerFocused -> ShellBack.ToHeader
    tab != TopTab.Home -> ShellBack.ToHome
    else -> ShellBack.Leave
}

/**
 * A top-level section: the [TvHeader] at the top of the safe area, the
 * section's [content] below it (20 dp under the header, full width: the
 * content keeps its own side and bottom margins, not the top one).
 *
 * Back, unless the content handles it first (a screen's own `BackHandler`
 * is registered after this one, so it wins): from the content to the
 * header, then from the header to Home, then out of the app ([shellBack]).
 */
@Composable
fun TopLevelShell(
    tab: TopTab,
    accountName: String,
    onSelect: (TopTab) -> Unit,
    onAccount: () -> Unit,
    content: @Composable () -> Unit,
) {
    val layout = IrisLayout.current
    val header = remember { FocusRequester() }
    var headerFocused by remember { mutableStateOf(false) }
    val back = shellBack(tab, headerFocused)
    BackHandler(enabled = back != ShellBack.Leave) {
        when (back) {
            ShellBack.ToHeader -> header.requestFocus()
            ShellBack.ToHome -> onSelect(TopTab.Home)
            ShellBack.Leave -> Unit
        }
    }
    Column(Modifier.fillMaxSize()) {
        Box(
            Modifier
                .padding(
                    start = layout.safeHorizontal,
                    end = layout.safeHorizontal,
                    top = layout.safeVertical,
                    bottom = IrisSpace.s7,
                )
                .focusRequester(header)
                .onFocusChanged { headerFocused = it.hasFocus }
                .focusGroup(),
        ) {
            TvHeader(
                current = tab,
                onSelect = { if (it != tab) onSelect(it) },
                accountName = accountName,
                onAccount = onAccount,
            )
        }
        Box(
            Modifier
                .weight(1f)
                .fillMaxWidth(),
        ) { content() }
    }
}
