package studio.kahn.iris.tv.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
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
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.TopTab
import studio.kahn.iris.tv.ui.components.TvHeader
import studio.kahn.iris.tv.ui.screens.home.CardAction
import studio.kahn.iris.tv.ui.screens.home.CardMenuHost
import studio.kahn.iris.tv.ui.screens.home.DiscoverEvent
import studio.kahn.iris.tv.ui.screens.home.DiscoverUiState
import studio.kahn.iris.tv.ui.screens.home.DiscoverViewModel
import studio.kahn.iris.tv.ui.screens.home.MoodModel
import studio.kahn.iris.tv.ui.screens.home.rememberCardFocus
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * Discover (web `routes/discover`): tonight's moods, movies or series (a board of tiles,
 * then one mood's titles), then every suggestion shelf. A suggestion names a title, not a
 * release: OK searches it, so releases are ranked and previewed before anything downloads.
 */
@Composable
fun DiscoverScreen(
    container: AppContainer,
    onSelectTab: (TopTab) -> Unit,
    onOpenSearch: (query: String) -> Unit,
    onOpenSettings: () -> Unit,
) {
    val vm = irisViewModel(container) { c, saved -> DiscoverViewModel(c, saved) }
    val state by vm.state.collectAsStateWithLifecycle()
    RepeatWhileStarted(Unit) { vm.refreshOnStart() }
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            when (event) {
                is DiscoverEvent.Search -> onOpenSearch(event.query)
            }
        }
    }
    DiscoverContent(
        state = state,
        onSelectTab = onSelectTab,
        onAccount = onOpenSettings,
        onKind = vm::setKind,
        onOpenMood = { vm.openMood(it.id) },
        onCloseMood = vm::closeMood,
        onCardAction = vm::onCardAction,
        onRetry = vm::retry,
    )
}

@Composable
fun DiscoverContent(
    state: DiscoverUiState,
    onSelectTab: (TopTab) -> Unit,
    onAccount: () -> Unit,
    onKind: (MediaKind) -> Unit,
    onOpenMood: (MoodModel) -> Unit,
    onCloseMood: () -> Unit,
    onCardAction: (String, CardAction) -> Unit,
    onRetry: () -> Unit,
) {
    val layout = IrisLayout.current
    val header = remember { FocusRequester() }
    val kindFocus = remember { FocusRequester() }
    val focus = rememberCardFocus()
    val tiles = remember { HashMap<String, FocusRequester>() }
    val tileFocus = { id: String -> tiles.getOrPut(id) { FocusRequester() } }
    var headerFocused by remember { mutableStateOf(false) }
    var lastMood by remember { mutableStateOf<String?>(null) }
    var focusPlaced by remember { mutableStateOf(false) }

    val mood = state.mood
    // The first focus: the first mood (or the open mood's first title), else the kind choice.
    val firstKey = mood?.cards?.valueOrNull?.firstOrNull()?.key
    val firstMood = state.board.valueOrNull?.firstOrNull()?.id
    LaunchedEffect(firstKey, firstMood, mood == null) {
        if (focusPlaced) return@LaunchedEffect
        val target = when {
            mood != null && firstKey != null -> focus.requester(firstKey)
            mood == null && firstMood != null -> tileFocus(firstMood)
            else -> return@LaunchedEffect
        }
        focusPlaced = runCatching { target.requestFocus() }.isSuccess
    }
    // Back from a mood's titles to the board, on the tile it came from.
    LaunchedEffect(mood?.id) {
        val id = mood?.id
        if (id != null) {
            lastMood = id
        } else {
            lastMood?.let { previous -> runCatching { tileFocus(previous).requestFocus() } }
            lastMood = null
        }
    }
    BackHandler(enabled = mood != null, onBack = onCloseMood)
    BackHandler(enabled = mood == null && !headerFocused) { runCatching { header.requestFocus() } }

    Column(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
    ) {
        val moodCols = moodColumns()
        val posterCols = posterColumns()
        LazyColumn(
            Modifier
                .weight(1f)
                .fillMaxWidth(),
            state = rememberLazyListState(),
            contentPadding = PaddingValues(top = layout.safeVertical, bottom = IrisSpace.s7),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s7),
        ) {
            item(key = "header", contentType = "header") {
                Column(
                    Modifier.padding(horizontal = layout.safeHorizontal),
                    verticalArrangement = Arrangement.spacedBy(IrisSpace.s7),
                ) {
                    TvHeader(
                        current = TopTab.Discover,
                        onSelect = onSelectTab,
                        accountName = state.account,
                        onAccount = onAccount,
                        modifier = Modifier
                            .focusRequester(header)
                            .onFocusChanged { headerFocused = it.hasFocus },
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5), verticalAlignment = Alignment.Bottom) {
                        Text("Discover", style = IrisType.page, color = IrisColor.ink)
                        Text(
                            "Tonight's moods and what is trending, checked against your trackers.",
                            style = IrisType.meta,
                            color = IrisColor.inkMuted,
                            modifier = Modifier.padding(bottom = 3.dp),
                        )
                    }
                }
            }
            moodsHead(state.kind, onKind, kindFocus)
            if (mood == null) {
                moodBoard(state.board, onOpenMood, onRetry, tileFocus, moodCols)
            } else {
                moodResults(mood, focus, onCloseMood, onCardAction, onRetry, posterCols)
            }
            item(key = "fy-head", contentType = "head") {
                Text(
                    "Suggested for you",
                    style = IrisType.panel,
                    color = IrisColor.ink,
                    modifier = Modifier.padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = IrisSpace.s5),
                )
            }
            forYouShelves(state.forYou, focus, onCardAction, onRetry)
        }
        Column(
            Modifier
                .fillMaxWidth()
                .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = IrisSpace.s2, bottom = 15.dp),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            state.notice?.let {
                StatusLine(it.text, tone = it.tone, modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
            }
            KeyHints(
                listOf(
                    KeyHint(Keys.OK, if (mood == null) "Open, find releases" else "Find releases"),
                    KeyHint(Keys.HOLD_OK, "Not interested"),
                    KeyHint(Keys.BACK, if (mood == null) "To the menu" else "To all moods"),
                ),
            )
        }
    }
    CardMenuHost(
        focus = focus,
        busy = state.busy,
        present = { key -> state.hasCard(key) },
        fallback = kindFocus,
        onCardAction = onCardAction,
    )
}

private fun DiscoverUiState.hasCard(key: String): Boolean =
    mood?.cards?.valueOrNull?.any { it.key == key } == true ||
        forYou.valueOrNull?.any { shelf -> shelf.cards.any { it.key == key } } == true
