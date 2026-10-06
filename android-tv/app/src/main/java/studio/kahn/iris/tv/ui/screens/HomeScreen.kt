package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.FooterLayout
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.ScreenFooter
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.nav.LocalShellHeader
import studio.kahn.iris.tv.ui.nav.ShellBackdrop
import studio.kahn.iris.tv.ui.nav.LocalShellTopInset
import studio.kahn.iris.tv.ui.screens.home.CardAction
import studio.kahn.iris.tv.ui.screens.home.CardMenuHost
import studio.kahn.iris.tv.ui.screens.home.HeroAction
import studio.kahn.iris.tv.ui.screens.home.HeroModel
import studio.kahn.iris.tv.ui.screens.home.HomeEvent
import studio.kahn.iris.tv.ui.screens.home.HomeRow
import studio.kahn.iris.tv.ui.screens.home.HomeUiState
import studio.kahn.iris.tv.ui.screens.home.HomeViewModel
import studio.kahn.iris.tv.ui.screens.home.Notice
import studio.kahn.iris.tv.ui.screens.home.rememberCardFocus
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.format.plural

/**
 * Home (TV.dc.html, web `routes/+page.svelte`): what to watch now (resume the last thing,
 * else the library's freshest title, else the tracker's featured release), what Iris is
 * doing right now, then the rows: Continue watching, the watchlist (fresh episodes first),
 * the suggestions, the library. The first-run preferences sheet opens over it when due.
 * The shell draws its header over the hero's still ([ShellBackdrop]); the rows start below
 * it ([LocalShellTopInset]).
 */
@Composable
fun HomeScreen(
    container: AppContainer,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onOpenCollection: (collectionId: String) -> Unit,
    onOpenSearch: (query: String?) -> Unit,
    onOpenLibrary: () -> Unit,
    onOpenDiscover: () -> Unit,
) {
    val vm = irisViewModel(container) { c, _ -> HomeViewModel(c) }
    val state by vm.state.collectAsStateWithLifecycle()
    RepeatWhileStarted(Unit) { vm.refreshWhileStarted() }
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            when (event) {
                is HomeEvent.Play -> onPlay(event.infohash, event.fileIdx)
                is HomeEvent.OpenCollection -> onOpenCollection(event.id)
                is HomeEvent.Search -> onOpenSearch(event.query)
                HomeEvent.OpenLibrary -> onOpenLibrary()
            }
        }
    }
    Box(Modifier.fillMaxSize()) {
        HomeContent(
            state = state,
            onHeroAction = vm::onHeroAction,
            onCardAction = vm::onCardAction,
            onRetry = vm::retry,
            onOpenDiscover = onOpenDiscover,
            onOpenLibrary = onOpenLibrary,
            onOpenSearch = { onOpenSearch(null) },
        )
        state.onboarding?.let { prefs ->
            OnboardingSheet(container, prefs, onClosed = vm::onboardingClosed)
        }
    }
}

@Composable
fun HomeContent(
    state: HomeUiState,
    onHeroAction: (HeroAction) -> Unit,
    onCardAction: (String, CardAction) -> Unit,
    onRetry: () -> Unit,
    onOpenDiscover: () -> Unit,
    onOpenLibrary: () -> Unit,
    onOpenSearch: () -> Unit,
) {
    val layout = IrisLayout.current
    val header = LocalShellHeader.current
    val heroFocus = remember { FocusRequester() }
    val focus = rememberCardFocus()
    val list = rememberLazyListState()
    var focusPlaced by remember { mutableStateOf(false) }

    // The first focus: the hero's main action, else the menu once the home knows it has no hero.
    val sheetOpen = state.onboarding != null
    LaunchedEffect(state.hero?.key, state.heroPending, sheetOpen) {
        if (focusPlaced || state.heroPending || sheetOpen) return@LaunchedEffect
        val target = if (state.hero != null) heroFocus else header ?: return@LaunchedEffect
        focusPlaced = runCatching { target.requestFocus() }.isSuccess
    }

    // No ground fill here: the header and the hero's still are drawn behind this content.
    Box(Modifier.fillMaxSize()) {
        val hero = state.hero
        if (hero != null) {
            ShellBackdrop {
                Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.TopEnd) {
                    HeroArt(hero.art, width = layout.width * HERO_ART_WIDTH, height = layout.height * HERO_ART_HEIGHT, list = list)
                }
            }
        }
        FooterLayout(footer = { Footer(state.notice, state.updateAvailable) }, Modifier.fillMaxSize()) { footer ->
            LazyColumn(
                Modifier
                    .padding(top = LocalShellTopInset.current, bottom = footer)
                    .fillMaxSize(),
                state = list,
                contentPadding = PaddingValues(bottom = IrisSpace.s7),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s7),
            ) {
                if (hero != null) {
                    item(key = "hero", contentType = "hero") {
                        Hero(hero, state.busy, heroFocus, onHeroAction, Modifier.padding(horizontal = layout.safeHorizontal))
                    }
                }
                item(key = "cw", contentType = "row") {
                    HomeRow(
                        title = "Continue watching",
                        cards = state.continueWatching,
                        focus = focus,
                        still = true,
                        onCardAction = onCardAction,
                        onRetry = onRetry,
                        emptyText = "Nothing to resume yet. What you start watching waits for you here.",
                    )
                }
                item(key = "now", contentType = "now") { RightNow(state.rightNow) }
                item(key = "watchlist", contentType = "row") {
                    HomeRow(
                        title = "Your watchlist",
                        meta = state.watchlistCount.takeIf { it > 0 }?.let { plural(it.toLong(), "series", "series") },
                        cards = state.watchlist,
                        focus = focus,
                        still = false,
                        onCardAction = onCardAction,
                        onRetry = onRetry,
                        emptyText = "No series followed yet.",
                        emptyHint = "Find a series in Search: getting an episode follows it.",
                        emptyActionLabel = "Search",
                        onEmptyAction = onOpenSearch,
                    )
                }
                items(state.forYou, key = { it.key }, contentType = { "row" }) { shelf ->
                    HomeRow(
                        title = shelf.title,
                        cards = Loadable.Ready(shelf.cards),
                        focus = focus,
                        still = false,
                        onCardAction = onCardAction,
                        onRetry = onRetry,
                        onSeeAll = onOpenDiscover,
                        hideEmpty = true,
                    )
                }
                item(key = "library", contentType = "row") {
                    HomeRow(
                        title = "Your library",
                        meta = state.libraryCount.takeIf { it > 0 }?.let { plural(it.toLong(), "title") },
                        cards = state.library,
                        focus = focus,
                        still = false,
                        onCardAction = onCardAction,
                        onRetry = onRetry,
                        onSeeAll = onOpenLibrary,
                        emptyText = "Nothing in the library yet.",
                        emptyHint = "Start a search to add your first title.",
                        emptyActionLabel = "Search",
                        onEmptyAction = onOpenSearch,
                    )
                }
                item(key = "tonight", contentType = "tonight") {
                    Row(
                        Modifier.padding(horizontal = layout.safeHorizontal),
                        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("Not sure what to watch tonight?", style = IrisType.body, color = IrisColor.inkMuted)
                        ActionButton("Pick a mood in Discover", onOpenDiscover, style = ActionStyle.Secondary, size = ActionSize.Small)
                    }
                }
            }
        }
    }
    CardMenuHost(
        focus = focus,
        busy = state.busy,
        present = { key -> state.hasCard(key) },
        fallback = header,
        onCardAction = onCardAction,
    )
}

private fun HomeUiState.hasCard(key: String): Boolean =
    sequenceOf(continueWatching.valueOrNull, watchlist.valueOrNull, library.valueOrNull)
        .plus(forYou.asSequence().map { it.cards })
        .any { list -> list?.any { it.key == key } == true }

// The board's hero still: 1100 x 620 of 1920 x 1080, in the top right corner.
private const val HERO_ART_WIDTH = 1100f / 1920f
private const val HERO_ART_HEIGHT = 620f / 1080f

/**
 * The hero's still in the screen's top right corner, under the header, fading into the
 * ground towards the words (left) and the rows (bottom). It is not in the [list] (whose rows
 * are clipped below the header), so it follows the list's scroll itself.
 */
@Composable
private fun HeroArt(url: String?, width: Dp, height: Dp, list: LazyListState, modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(width, height)
            .graphicsLayer {
                val atTop = list.firstVisibleItemIndex == 0
                translationY = if (atTop) -list.firstVisibleItemScrollOffset.toFloat() else 0f
                alpha = if (atTop) 1f else 0f
            }
            .background(IrisColor.art)
            .clearAndSetSemantics {},
    ) {
        if (!url.isNullOrBlank()) {
            val context = LocalContext.current
            val density = LocalDensity.current
            val request = remember(url, width, height, density) {
                with(density) {
                    ImageRequest.Builder(context).data(url).size(width.roundToPx(), height.roundToPx()).build()
                }
            }
            AsyncImage(
                model = request,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.matchParentSize(),
            )
        }
        Box(
            Modifier
                .matchParentSize()
                .background(Brush.horizontalGradient(0f to IrisColor.ground, FADE_LEFT to IrisColor.ground.copy(alpha = 0f))),
        )
        Box(
            Modifier
                .matchParentSize()
                .background(Brush.verticalGradient(FADE_BOTTOM to IrisColor.ground.copy(alpha = 0f), 1f to IrisColor.ground)),
        )
    }
}

private const val FADE_LEFT = 0.45f
private const val FADE_BOTTOM = 0.6f

@Composable
private fun Hero(
    hero: HeroModel,
    busy: String?,
    primaryFocus: FocusRequester,
    onAction: (HeroAction) -> Unit,
    modifier: Modifier = Modifier,
) {
    val layout = IrisLayout.current
    val compact = layout.height < COMPACT_HEIGHT
    Column(
        modifier
            .widthIn(max = 430.dp)
            .padding(top = if (compact) 0.dp else IrisSpace.s5),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
    ) {
        Eyebrow(hero.eyebrow)
        Text(
            hero.title,
            style = if (compact) IrisType.title else IrisType.hero,
            color = IrisColor.ink,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        if (hero.meta != null) Text(hero.meta, style = IrisType.metaLarge, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (hero.overview != null) {
            Text(
                hero.overview,
                style = IrisType.body,
                color = IrisColor.inkMuted,
                maxLines = if (compact) 2 else 3,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.widthIn(max = 380.dp),
            )
        }
        Row(
            Modifier
                .padding(top = IrisSpace.s1)
                .focusRestorer(primaryFocus)
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            hero.actions.forEachIndexed { i, button ->
                ActionButton(
                    button.label,
                    onClick = { onAction(button.action) },
                    style = if (i == 0) ActionStyle.Primary else ActionStyle.Secondary,
                    icon = if (i == 0) Icons.Rounded.PlayArrow else null,
                    busy = button.busyKey != null && button.busyKey == busy,
                    busyText = button.busyLabel,
                    modifier = if (i == 0) Modifier.focusRequester(primaryFocus) else Modifier,
                )
            }
        }
        if (hero.languages != null) Text(hero.languages, style = IrisType.meta, color = IrisColor.inkMuted)
    }
}

private val COMPACT_HEIGHT = 480.dp

/** "Right now": what the house's Iris is doing, in a few words. Nothing while nothing is known. */
@Composable
private fun RightNow(facts: Loadable<List<String>>) {
    val list = facts.valueOrNull
    val failed = facts as? Loadable.Failed
    if (list.isNullOrEmpty() && failed == null) return
    Column(
        Modifier
            .padding(horizontal = IrisLayout.current.safeHorizontal)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
    ) {
        Eyebrow("Right now")
        if (failed != null) {
            StatusLine("Couldn't read what Iris is doing: ${failed.error.message}", tone = StatusTone.Down)
        } else {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s2),
            ) {
                list.orEmpty().forEach { Chip(it) }
            }
            facts.errorOrNull?.let { StaleNotice(it) }
        }
    }
}

@Composable
private fun Footer(notice: Notice?, updateAvailable: Boolean) {
    ScreenFooter(HOME_HINTS, trailing = if (updateAvailable) "An app update is waiting in Settings" else null) {
        if (notice != null) {
            StatusLine(notice.text, tone = notice.tone, modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
        }
    }
}

private val HOME_HINTS = listOf(
    KeyHint(Keys.OK, "Play"),
    KeyHint(Keys.HOLD_OK, "Remove, mark watched"),
    KeyHint(Keys.BACK, "To the menu"),
)
