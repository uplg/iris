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
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.rememberFocusReturn
import studio.kahn.iris.tv.ui.components.focusReturn
import studio.kahn.iris.tv.ui.components.FooterLayout
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.ScreenFooter
import studio.kahn.iris.tv.ui.components.StatusLine
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
import studio.kahn.iris.tv.ui.components.NoticeLine

/**
 * Discover (web `routes/discover`): tonight's moods, movies or series (a board of tiles,
 * then one mood's titles), then every suggestion shelf. A suggestion names a title, not a
 * release: OK searches it, so releases are ranked and previewed before anything downloads.
 */
@Composable
fun DiscoverScreen(
    container: AppContainer,
    onOpenSearch: (query: String) -> Unit,
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
    onKind: (MediaKind) -> Unit,
    onOpenMood: (MoodModel) -> Unit,
    onCloseMood: () -> Unit,
    onCardAction: (String, CardAction) -> Unit,
    onRetry: () -> Unit,
) {
    val layout = IrisLayout.current
    val kindFocus = remember { FocusRequester() }
    val focus = rememberCardFocus(fallback = kindFocus)
    val tiles = rememberFocusReturn()
    val tileFocus = { id: String -> tiles.requester(id) }
    val tileModifier = { id: String -> Modifier.focusReturn(tiles, id) }
    var lastMood by remember { mutableStateOf<String?>(null) }
    var focusPlaced by remember { mutableStateOf(false) }

    val mood = state.mood
    // The first focus: the first mood (or the open mood's first title), else the kind choice.
    val firstKey = mood?.cards?.valueOrNull?.firstOrNull()?.key
    val firstMood = state.board.valueOrNull?.firstOrNull()?.id
    LaunchedEffect(firstKey, firstMood, mood == null) {
        if (focusPlaced) return@LaunchedEffect
        // Back on this screen: the card or the mood left.
        withFrameNanos { }
        if (focus.focusLast() || (mood == null && tiles.focusLast())) {
            focusPlaced = true
            return@LaunchedEffect
        }
        val target = when {
            mood != null && firstKey != null -> focus.requester(firstKey)
            mood == null && firstMood != null -> tileFocus(firstMood)
            else -> return@LaunchedEffect
        }
        focusPlaced = runCatching { target.requestFocus() }.getOrDefault(false)
    }
    // Back from a mood's titles to the board, on the tile it came from.
    LaunchedEffect(mood?.id) {
        val id = mood?.id
        if (id != null) {
            lastMood = id
        } else {
            lastMood?.let { previous -> tiles.returnTo(previous) }
            lastMood = null
        }
    }
    BackHandler(enabled = mood != null, onBack = onCloseMood)

    val footer: @Composable () -> Unit = {
        ScreenFooter(
            listOfNotNull(
                KeyHint(Keys.OK, if (mood == null) "Open, find releases" else "Find releases"),
                // Mood tiles have no menu: the hold only means something on titles.
                if (mood != null || !state.forYou.valueOrNull.isNullOrEmpty()) KeyHint(Keys.HOLD_OK, "Not interested") else null,
                KeyHint(Keys.BACK, if (mood == null) "To the menu" else "To all moods"),
            ),
        ) { NoticeLine(state.notice) }
    }
    FooterLayout(
        footer = footer,
        modifier = Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
    ) { bottom ->
        val moodCols = moodColumns()
        val posterCols = posterColumns()
        LazyColumn(
            Modifier
                .fillMaxSize()
                .padding(bottom = bottom),
            state = rememberLazyListState(),
            contentPadding = PaddingValues(bottom = IrisSpace.s7),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s7),
        ) {
            item(key = "title", contentType = "title") {
                Row(
                    Modifier.padding(horizontal = layout.safeHorizontal),
                    horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
                    verticalAlignment = Alignment.Bottom,
                ) {
                    Text("Discover", style = IrisType.page, color = IrisColor.ink)
                    Text(
                        "Tonight's moods and what is trending, checked against your trackers.",
                        style = IrisType.meta,
                        color = IrisColor.inkMuted,
                        modifier = Modifier.padding(bottom = 3.dp),
                    )
                }
            }
            moodsHead(state.kind, onKind, kindFocus)
            if (mood == null) {
                moodBoard(state.board, onOpenMood, onRetry, tileModifier, moodCols)
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
    }
    CardMenuHost(
        focus = focus,
        busy = state.busy,
        onCardAction = onCardAction,
    )
}
