package studio.kahn.iris.tv.ui.nav

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.State
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.unit.dp
import androidx.navigation.NavBackStackEntry
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
    val updateAvailable: State<Boolean>,
    val onSelect: (TopTab) -> Unit,
    val onAccount: () -> Unit,
)

/**
 * A top-level destination ([tab] in the header): `composable<T>` with its
 * content inside the [TopLevelShell]. [headerOverContent]: the content takes
 * the whole screen, the header over a [ShellBackdrop] (Home's hero still).
 */
inline fun <reified T : Any> NavGraphBuilder.section(
    tab: TopTab,
    host: ShellHost,
    headerOverContent: Boolean = false,
    noinline content: @Composable (NavBackStackEntry) -> Unit,
) {
    composable<T> { entry ->
        TopLevelShell(
            tab = tab,
            accountName = host.accountName.value,
            updateAvailable = host.updateAvailable.value,
            onSelect = host.onSelect,
            onAccount = host.onAccount,
            headerOverContent = headerOverContent,
        ) { content(entry) }
    }
}

/** The header of the section around this content: where focus goes when nothing of the content can take it. */
val LocalShellHeader = staticCompositionLocalOf<FocusRequester?> { null }

/**
 * How far from the top the content of a `headerOverContent` section starts
 * to clear the header (the safe margin, the header, its gap); 0 dp in the
 * other sections, whose content already starts below the header.
 */
val LocalShellTopInset = compositionLocalOf { 0.dp }

private enum class ShellSlot { Backdrop, Header, Content }

@Stable
class ShellBackdropHolder {
    var content by mutableStateOf<(@Composable () -> Unit)?>(null)
}

val LocalShellBackdrop = staticCompositionLocalOf<ShellBackdropHolder?> { null }

/**
 * Draws [content] behind the header of a `headerOverContent` section, in the
 * screen's top right corner (Home's hero still), for as long as this stays
 * composed. Outside such a section it draws in place.
 */
@Composable
fun ShellBackdrop(content: @Composable () -> Unit) {
    val holder = LocalShellBackdrop.current
    if (holder == null) {
        content()
        return
    }
    val current by rememberUpdatedState(content)
    DisposableEffect(holder) {
        holder.content = { current() }
        onDispose { holder.content = null }
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
 * content keeps its own side and bottom margins, not the top one). With
 * [headerOverContent] the content fills the screen, clears the header with
 * [LocalShellTopInset] and may put a [ShellBackdrop] behind it.
 *
 * Back, unless the content handles it first (a screen's own `BackHandler`
 * is registered after this one, so it wins): from the content to the
 * header, then from the header to Home, then out of the app ([shellBack]).
 */
@Composable
fun TopLevelShell(
    tab: TopTab,
    accountName: String,
    updateAvailable: Boolean = false,
    onSelect: (TopTab) -> Unit,
    onAccount: () -> Unit,
    headerOverContent: Boolean = false,
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
    val headerBox: @Composable (Modifier) -> Unit = { modifier ->
        Box(
            modifier
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
                updateAvailable = updateAvailable,
            )
        }
    }
    val backdrop = remember { ShellBackdropHolder() }
    CompositionLocalProvider(LocalShellHeader provides header, LocalShellBackdrop provides backdrop.takeIf { headerOverContent }) {
        if (headerOverContent) {
            // Drawn bottom to top: the content's backdrop, the header, the content (its dialogs
            // cover the header). The header is measured first so the content knows its inset.
            SubcomposeLayout(Modifier.fillMaxSize()) { constraints ->
                val loose = constraints.copy(minWidth = 0, minHeight = 0)
                val back = subcompose(ShellSlot.Backdrop) { backdrop.content?.invoke() }.map { it.measure(loose) }
                val top = subcompose(ShellSlot.Header) { headerBox(Modifier.fillMaxWidth()) }.map { it.measure(loose) }
                val inset = (top.maxOfOrNull { it.height } ?: 0).toDp()
                val body = subcompose(ShellSlot.Content) {
                    CompositionLocalProvider(LocalShellTopInset provides inset) { content() }
                }.map { it.measure(constraints) }
                layout(constraints.maxWidth, constraints.maxHeight) {
                    back.forEach { it.place(constraints.maxWidth - it.width, 0) }
                    top.forEach { it.place(0, 0) }
                    body.forEach { it.place(0, 0) }
                }
            }
        } else {
            Column(Modifier.fillMaxSize()) {
                headerBox(Modifier)
                Box(
                    Modifier
                        .weight(1f)
                        .fillMaxWidth(),
                ) { content() }
            }
        }
    }
}
