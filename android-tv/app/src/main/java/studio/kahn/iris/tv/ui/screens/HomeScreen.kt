package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
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
import androidx.compose.material.icons.rounded.SystemUpdate
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
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
import kotlinx.coroutines.launch
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.AppUpdater
import studio.kahn.iris.tv.data.UpdateNotice
import studio.kahn.iris.tv.data.updateNotice
import studio.kahn.iris.tv.ui.components.FramedBlock
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.update.UpdateActions
import studio.kahn.iris.tv.ui.update.UpdateButton
import studio.kahn.iris.tv.ui.update.UpdateProgress
import studio.kahn.iris.tv.ui.update.actions
import studio.kahn.iris.tv.ui.update.versionsLine
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
import studio.kahn.iris.tv.ui.screens.home.rememberCardFocus
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.components.Notice
import studio.kahn.iris.tv.ui.components.NoticeLine

/**
 * Home (TV.dc.html, web `routes/+page.svelte`): what to watch now (resume the last thing,
 * else the library's freshest title, else the tracker's featured release), what Iris is
 * doing right now, then the rows: Continue watching, the watchlist (fresh episodes first),
 * the library, the suggestions. The first-run preferences sheet opens over it when due.
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
    val updates by container.updates.state.collectAsStateWithLifecycle()
    val updateActions = remember(container) { container.updates.actions() }
    Box(Modifier.fillMaxSize()) {
        HomeContent(
            state = state,
            update = updateNotice(updates),
            updateActions = updateActions,
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
    update: UpdateNotice? = null,
    updateActions: UpdateActions = UpdateActions(),
) {
    val layout = IrisLayout.current
    val header = LocalShellHeader.current
    val heroFocus = remember { FocusRequester() }
    val focus = rememberCardFocus(fallback = header)
    val list = rememberLazyListState()
    var focusPlaced by remember { mutableStateOf(false) }
    // A card focused during this visit before the hero came (the platform's default focus lands
    // on the first focusable) is not a card left: only a return goes back to one.
    val returning = remember { focus.returns.last != null }
    val heroIndex by rememberUpdatedState(if (update != null) 1 else 0)

    // The first focus: back on this screen the card left, else the hero's main action, else the
    // menu once the home knows it has no hero. The onboarding sheet closing puts it back too.
    val sheetOpen = state.onboarding != null
    var sheetWasOpen by remember { mutableStateOf(false) }
    LaunchedEffect(sheetOpen) {
        if (sheetOpen) {
            sheetWasOpen = true
        } else if (sheetWasOpen) {
            sheetWasOpen = false
            focusPlaced = false
        }
    }
    LaunchedEffect(state.hero?.key, state.heroPending, sheetOpen, focusPlaced) {
        if (focusPlaced || sheetOpen) return@LaunchedEffect
        withFrameNanos { }
        if (returning && focus.focusLast()) {
            focusPlaced = true
            return@LaunchedEffect
        }
        if (state.heroPending) return@LaunchedEffect
        if (state.hero == null) {
            focusPlaced = header != null && runCatching { header.requestFocus() }.getOrDefault(false)
            return@LaunchedEffect
        }
        // The hero comes in above rows composed while it was pending, and the list keeps its
        // first visible row where it was: the hero sits above the viewport, not composed, and
        // focusing it fails. The list goes back to its top first; when an update banner leaves
        // no room for the whole hero, the banner scrolls out instead.
        list.scrollToItem(0)
        withFrameNanos { }
        val info = list.layoutInfo
        val shown = info.visibleItemsInfo.firstOrNull { it.key == HERO_KEY }
        if (shown == null || shown.offset + shown.size > info.viewportEndOffset) {
            list.scrollToItem(heroIndex)
            withFrameNanos { }
        }
        focusPlaced = runCatching { heroFocus.requestFocus() }.getOrDefault(false)
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
        FooterLayout(footer = { Footer(state.notice) }, Modifier.fillMaxSize()) { footer ->
            LazyColumn(
                Modifier
                    .padding(top = LocalShellTopInset.current, bottom = footer)
                    .fillMaxSize(),
                state = list,
                contentPadding = PaddingValues(bottom = IrisSpace.s7),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s7),
            ) {
                // Above the hero, out of the first focus's way: D-pad up or a tap reaches it.
                if (update != null) {
                    item(key = "update", contentType = "update") {
                        UpdateBanner(update, updateActions, Modifier.padding(horizontal = layout.safeHorizontal))
                    }
                }
                if (hero != null) {
                    item(key = HERO_KEY, contentType = "hero") {
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
        onCardAction = onCardAction,
    )
}

private const val HERO_KEY = "hero"

// The board's hero still: 1100 x 620 of 1920 x 1080, in the top right corner.
private const val HERO_ART_WIDTH = 1100f / 1920f
private const val HERO_ART_HEIGHT = 620f / 1080f

/**
 * The hero's still in the screen's top right corner, under the header, fading into the
 * ground towards the words (left) and the rows (bottom). It is not in the [list] (whose rows
 * are clipped below the header), so it follows the hero's item itself: up with it once it
 * reaches the top, gone once it has scrolled out.
 */
@Composable
private fun HeroArt(url: String?, width: Dp, height: Dp, list: LazyListState, modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(width, height)
            .graphicsLayer {
                val hero = list.layoutInfo.visibleItemsInfo.firstOrNull { it.key == HERO_KEY }
                translationY = hero?.offset?.coerceAtMost(0)?.toFloat() ?: 0f
                alpha = if (hero != null) 1f else 0f
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
    // Focus coming back up from the rows brings only the button into view: the title above it
    // would stay under the header (nothing above the buttons takes focus to scroll it back).
    val whole = remember { BringIntoViewRequester() }
    val scope = rememberCoroutineScope()
    Column(
        modifier
            .bringIntoViewRequester(whole)
            .widthIn(max = 430.dp)
            .padding(top = if (compact) 0.dp else IrisSpace.s5),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
    ) {
        Eyebrow(hero.eyebrow)
        Text(
            hero.title,
            style = IrisType.titleFor(hero.title, if (compact) IrisType.title else IrisType.hero),
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
                .onFocusChanged { if (it.hasFocus) scope.launch { whole.bringIntoView() } }
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

/**
 * A newer Iris, one press from installing (the same download as Settings → App update), or
 * put off until the app starts again; then the download as it goes.
 */
@Composable
private fun UpdateBanner(notice: UpdateNotice, actions: UpdateActions, modifier: Modifier = Modifier) {
    val progress = notice.progress
    val title = when {
        progress is AppUpdater.Progress.Ready -> notice.latest?.let { "Iris $it is downloaded" } ?: "The Iris update is downloaded"
        progress != null && progress !is AppUpdater.Progress.Failed -> notice.latest?.let { "Updating Iris to $it" } ?: "Updating Iris"
        else -> versionsLine(notice.latest, notice.installed)
    }
    val canPutOff = notice.latest != null && (progress == null || progress is AppUpdater.Progress.Failed)
    FramedBlock(modifier.widthIn(max = UPDATE_BANNER_MAX)) {
        Row(
            Modifier.focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Rounded.SystemUpdate, contentDescription = null, tint = IrisColor.accent, modifier = Modifier.size(IrisSize.icon))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
                Text(title, style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                UpdateProgress(progress)
            }
            UpdateButton(progress, actions, size = ActionSize.Small)
            if (canPutOff) ActionButton("Later", actions.later, style = ActionStyle.Secondary, size = ActionSize.Small)
        }
    }
}

private val UPDATE_BANNER_MAX = 760.dp

@Composable
private fun Footer(notice: Notice?) {
    ScreenFooter(HOME_HINTS) {
        NoticeLine(notice)
    }
}

private val HOME_HINTS = listOf(
    KeyHint(Keys.OK, "Play"),
    KeyHint(Keys.HOLD_OK, "Remove, mark watched"),
    KeyHint(Keys.BACK, "To the menu"),
)
