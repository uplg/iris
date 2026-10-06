package studio.kahn.iris.tv.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Search
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableIntState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.ConfirmDialog
import studio.kahn.iris.tv.ui.components.EmptyState
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.PanelLabel
import studio.kahn.iris.tv.ui.components.Pill
import studio.kahn.iris.tv.ui.components.PillChoice
import studio.kahn.iris.tv.ui.components.PosterCard
import studio.kahn.iris.tv.ui.components.PosterGrid
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.TextInput
import studio.kahn.iris.tv.ui.screens.library.ChosenPanelOptions
import studio.kahn.iris.tv.ui.screens.library.DownloadsUi
import studio.kahn.iris.tv.ui.screens.library.FocusKeys
import studio.kahn.iris.tv.ui.screens.library.LibraryUiState
import studio.kahn.iris.tv.ui.screens.library.LibraryView
import studio.kahn.iris.tv.ui.screens.library.LibraryViewModel
import studio.kahn.iris.tv.ui.screens.library.NoticeLine
import studio.kahn.iris.tv.ui.screens.library.ReleaseActions
import studio.kahn.iris.tv.ui.screens.library.ReleaseItem
import studio.kahn.iris.tv.ui.screens.library.ReleaseRow
import studio.kahn.iris.tv.ui.screens.library.neighbourOf
import studio.kahn.iris.tv.ui.screens.library.ShowFilter
import studio.kahn.iris.tv.ui.screens.library.Sort
import studio.kahn.iris.tv.ui.screens.library.TitleCard
import studio.kahn.iris.tv.ui.screens.library.TitleFilters
import studio.kahn.iris.tv.ui.screens.library.TitlesUi
import studio.kahn.iris.tv.ui.screens.library.TypeFilter
import studio.kahn.iris.tv.ui.screens.library.cardTone
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** Everything the library screen hands back to its ViewModel and to navigation. */
@Immutable
data class LibraryActions(
    val onView: (LibraryView) -> Unit = {},
    val onFilters: (TitleFilters) -> Unit = {},
    val onReleaseQuery: (String) -> Unit = {},
    val onOpenTitle: (TitleCard) -> Unit = {},
    val onHide: (TitleCard) -> Unit = {},
    val onRelease: ReleaseActions = ReleaseActions(),
    val onRetry: () -> Unit = {},
)

/**
 * The library (TVLibrary board, web `/library`): Titles (a poster grid with type and state
 * filters, a find field and a sort) or Downloads and seeding (every release by what it is
 * doing, with play, pause or resume and delete). In the Library section the shell draws the
 * header above it; the `Torrents` route opens it alone ([initialView] Downloads), with its own
 * top margin.
 */
@Composable
fun LibraryScreen(
    container: AppContainer,
    onOpenCollection: (collectionId: String) -> Unit,
    onOpenTorrent: (infohash: String) -> Unit,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    initialView: LibraryView = LibraryView.Titles,
) {
    val vm = irisViewModel(container) { c, _ -> LibraryViewModel(c, initialView) }
    val state by vm.state.collectAsStateWithLifecycle()
    RepeatWhileStarted(Unit) { vm.pollWhileStarted() }
    LibraryContent(
        state = state,
        lastOpened = vm.lastOpened,
        standalone = initialView == LibraryView.Downloads,
        actions = LibraryActions(
            onView = vm::choose,
            onFilters = vm::setFilters,
            onReleaseQuery = vm::setReleaseQuery,
            onOpenTitle = {
                vm.lastOpened = it.id
                onOpenCollection(it.id)
            },
            onHide = vm::hide,
            onRelease = ReleaseActions(
                onPlay = onPlay,
                onFiles = onOpenTorrent,
                onOpenTitle = onOpenCollection,
                onPause = vm::pause,
                onResume = vm::resume,
                onDelete = vm::delete,
            ),
            onRetry = vm::retry,
        ),
    )
}

private enum class Panel { None, Find, Sort, FindRelease }

@Composable
fun LibraryContent(
    state: LibraryUiState,
    actions: LibraryActions,
    lastOpened: String? = null,
    standalone: Boolean = false,
) {
    val layout = IrisLayout.current
    val top = remember { FocusRequester() }
    var contentFocused by remember { mutableStateOf(false) }
    var topFocused by remember { mutableStateOf(false) }
    var panel by remember { mutableStateOf(Panel.None) }
    var hiding by remember { mutableStateOf<TitleCard?>(null) }
    var deleting by remember { mutableStateOf<ReleaseRow?>(null) }
    val focusedIndex = remember { mutableIntStateOf(-1) }
    val grid = rememberLazyGridState()
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val keys = remember { FocusKeys() }
    // Where focus goes back once a dialog or a panel closes: the first of these keys on screen.
    var refocus by remember { mutableStateOf<List<String>?>(null) }
    LaunchedEffect(refocus) {
        val wanted = refocus ?: return@LaunchedEffect
        refocus = null
        if (!keys.focus(*wanted.toTypedArray())) runCatching { top.requestFocus() }
    }

    // The Titles view places its own focus (the title opened last); Downloads starts at the top.
    LaunchedEffect(Unit) {
        if (state.view != LibraryView.Downloads) return@LaunchedEffect
        snapshotFlow { list.layoutInfo.visibleItemsInfo.isNotEmpty() }.first { it }
        runCatching { top.requestFocus() }
    }
    // Back from deep in the view goes to its top first; from there the shell (or the back stack) takes over.
    BackHandler(enabled = contentFocused && !topFocused && panel == Panel.None && hiding == null && deleting == null) {
        scope.launch {
            if (state.view == LibraryView.Titles) grid.scrollToItem(0) else list.scrollToItem(0)
            runCatching { top.requestFocus() }
        }
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
    ) {
        Column(
            Modifier
                .fillMaxSize()
                .padding(top = if (standalone) layout.safeVertical else 0.dp)
                .onFocusChanged { contentFocused = it.hasFocus },
        ) {
            val heading: @Composable () -> Unit = {
                PageHeading(
                    state = state,
                    onView = actions.onView,
                    modifier = Modifier
                        .focusRequester(top)
                        .onFocusChanged { topFocused = it.hasFocus },
                )
            }
            when (state.view) {
                LibraryView.Titles -> TitlesPane(
                    state = state,
                    grid = grid,
                    heading = heading,
                    lastOpened = lastOpened,
                    focusedIndex = focusedIndex,
                    top = top,
                    keys = keys,
                    actions = actions,
                    onFind = { panel = Panel.Find },
                    onSort = { panel = Panel.Sort },
                    onHold = { hiding = it },
                )
                LibraryView.Downloads -> DownloadsPane(
                    state = state,
                    list = list,
                    keys = keys,
                    heading = heading,
                    actions = actions.copy(onRelease = actions.onRelease.copy(onDelete = { deleting = it })),
                    onFind = { panel = Panel.FindRelease },
                )
            }
        }
        LibraryHints(state, focusedIndex, Modifier.align(Alignment.BottomStart))

        when (panel) {
            Panel.None -> Unit
            Panel.Find, Panel.Sort -> FindAndSortPanel(
                state = state,
                focusSort = panel == Panel.Sort,
                onFilters = actions.onFilters,
                onDismiss = {
                    panel = Panel.None
                    refocus = listOf(FIND_KEY)
                },
            )
            Panel.FindRelease -> FindReleasePanel(
                query = state.releaseQuery,
                onQuery = actions.onReleaseQuery,
                onDismiss = {
                    panel = Panel.None
                    refocus = listOf(FIND_KEY)
                },
            )
        }
        hiding?.let { card ->
            ConfirmDialog(
                eyebrow = "Hide from my library",
                title = "Hide ${card.title}?",
                body = "Hidden for you only. Your watch history is kept, and the title comes back if you watch it again.",
                confirmLabel = "Hide",
                onConfirm = {
                    hiding = null
                    actions.onHide(card)
                    val ids = state.titles.valueOrNull?.cards?.map { it.id }.orEmpty()
                    refocus = listOfNotNull(neighbourOf(ids, card.id), card.id)
                },
                onCancel = {
                    hiding = null
                    refocus = listOf(card.id)
                },
            )
        }
        deleting?.let { row ->
            ConfirmDialog(
                eyebrow = "Delete ${row.title}",
                title = "Delete ${row.release}?",
                body = row.deleteBody,
                confirmLabel = "Delete release",
                onConfirm = {
                    deleting = null
                    actions.onRelease.onDelete(row)
                    val ids = state.downloads.valueOrNull?.groups?.flatMap { g -> g.rows.map { it.infohash } }.orEmpty()
                    refocus = listOfNotNull(neighbourOf(ids, row.infohash), row.infohash)
                },
                onCancel = {
                    deleting = null
                    refocus = listOf(row.infohash)
                },
            )
        }
    }
}

/** `Library` and its facts, the action notice, and the view choice. */
@Composable
private fun PageHeading(state: LibraryUiState, onView: (LibraryView) -> Unit, modifier: Modifier = Modifier) {
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            SectionTitle(
                "Library",
                meta = state.facts,
                style = IrisType.page,
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(IrisSpace.s6))
            PillChoice(
                options = LibraryView.entries,
                selected = state.view,
                onSelect = onView,
                label = { it.label },
                modifier = modifier,
            )
        }
        NoticeLine(state.notice)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun TitlesPane(
    state: LibraryUiState,
    grid: LazyGridState,
    heading: @Composable () -> Unit,
    lastOpened: String?,
    focusedIndex: MutableIntState,
    top: FocusRequester,
    keys: FocusKeys,
    actions: LibraryActions,
    onFind: () -> Unit,
    onSort: () -> Unit,
    onHold: (TitleCard) -> Unit,
) {
    val layout = IrisLayout.current
    val titles = state.titles
    val ui = titles.valueOrNull
    if (ui == null || ui.all == 0) {
        Column(
            Modifier
                .fillMaxSize()
                .padding(horizontal = layout.safeHorizontal)
                .padding(top = IrisSpace.s1, bottom = FOOTER),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s6),
        ) {
            heading()
            when {
                titles is Loadable.Failed -> ErrorState(titles.error.message, actions.onRetry)
                ui == null -> LoadingState(label = "Loading the library…")
                else -> EmptyState("Nothing in the library yet", body = "Search for a title to add the first one.")
            }
        }
        return
    }
    val indexOf = remember(ui.cards) { ui.cards.withIndex().associate { it.value.id to it.index } }
    val target = ui.cards.indexOfFirst { it.id == lastOpened }.takeIf { it >= 0 } ?: 0
    var placed by remember { mutableStateOf(false) }
    // A phone shows a row at most: entering there starts at the top, not on a poster.
    val startOnCard = lastOpened != null || layout.height >= 500.dp
    LaunchedEffect(ui.cards.isNotEmpty()) {
        if (placed || ui.cards.isEmpty()) return@LaunchedEffect
        placed = true
        if (!startOnCard) {
            runCatching { top.requestFocus() }
            return@LaunchedEffect
        }
        val index = target + HEADER_ITEMS
        if (grid.layoutInfo.visibleItemsInfo.none { it.index == index }) grid.scrollToItem(index)
        snapshotFlow { grid.layoutInfo.visibleItemsInfo.any { it.index == index } }.first { it }
        keys.focus(ui.cards[target].id)
    }
    PosterGrid(
        items = ui.cards,
        key = { it.id },
        state = grid,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(
            start = layout.safeHorizontal,
            end = layout.safeHorizontal,
            top = IrisSpace.s1,
            bottom = FOOTER + IrisSpace.s6,
        ),
        header = {
            item(key = "heading", span = { GridItemSpan(maxLineSpan) }) { heading() }
            item(key = "filters", span = { GridItemSpan(maxLineSpan) }) {
                Filters(state, ui, actions, keys, onFind, onSort)
            }
        },
    ) { card ->
        val index = indexOf[card.id] ?: 0
        PosterCard(
            title = card.title,
            imageUrl = card.posterUrl,
            onClick = { actions.onOpenTitle(card) },
            onLongClick = if (card.ghost) ({ onHold(card) }) else null,
            width = null,
            kind = card.kind,
            progress = card.progress,
            meta = card.meta,
            status = card.status.text,
            statusTone = card.status.tone.cardTone(),
            modifier = Modifier
                .focusRequester(keys.of(card.id))
                .onFocusChanged { if (it.hasFocus) focusedIndex.intValue = index },
        )
    }
}

private const val HEADER_ITEMS = 2
private const val FIND_KEY = "find"
private val FOOTER = 46.dp

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Filters(
    state: LibraryUiState,
    ui: TitlesUi,
    actions: LibraryActions,
    keys: FocusKeys,
    onFind: () -> Unit,
    onSort: () -> Unit,
) {
    val f = state.filters
    Column(Modifier.padding(bottom = IrisSpace.s2), verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        FlowRow(
            Modifier.focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            PillChoice(
                options = TypeFilter.entries,
                selected = f.type,
                onSelect = { actions.onFilters(f.copy(type = it)) },
                label = { it.label },
            )
            if (ui.showChoices.size > 1) {
                Box(
                    Modifier
                        .width(1.dp)
                        .height(IrisSize.chip)
                        .background(IrisColor.line),
                )
                PillChoice(
                    options = ui.showChoices.map { it.first },
                    selected = ui.showing,
                    onSelect = { actions.onFilters(f.copy(show = it)) },
                    label = { choice -> ui.showChoices.first { it.first == choice }.second },
                )
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(ui.countWords, style = IrisType.meta, color = IrisColor.inkMuted)
            if (f.filtered) {
                Pill("Clear filters", selected = false, onClick = { actions.onFilters(f.copy(query = "", type = TypeFilter.All, show = ShowFilter.All)) })
            }
            state.titles.errorOrNull?.let { StaleNotice(it) }
            Spacer(Modifier.weight(1f))
            Pill(
                text = if (f.query.isBlank()) "Find a title" else "Finding “${f.query.trim()}”",
                selected = f.query.isNotBlank(),
                onClick = onFind,
                modifier = Modifier.focusRequester(keys.of(FIND_KEY)),
            )
            Pill(text = f.sort.words, selected = false, onClick = onSort)
        }
        if (ui.cards.isEmpty()) {
            EmptyState(
                "No title matches these filters",
                modifier = Modifier.height(180.dp),
                actionLabel = "Clear filters",
                onAction = { actions.onFilters(f.copy(query = "", type = TypeFilter.All, show = ShowFilter.All)) },
            )
        }
    }
}

@Composable
private fun DownloadsPane(
    state: LibraryUiState,
    list: LazyListState,
    keys: FocusKeys,
    heading: @Composable () -> Unit,
    actions: LibraryActions,
    onFind: () -> Unit,
) {
    val layout = IrisLayout.current
    val downloads = state.downloads
    val ui: DownloadsUi? = downloads.valueOrNull
    LazyColumn(
        state = list,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(
            start = layout.safeHorizontal,
            end = layout.safeHorizontal,
            top = IrisSpace.s1,
            bottom = FOOTER + IrisSpace.s6,
        ),
    ) {
        item(key = "heading") { heading() }
        when {
            downloads is Loadable.Failed -> item(key = "failed") {
                ErrorState(downloads.error.message, actions.onRetry, Modifier.height(240.dp))
            }
            ui == null -> item(key = "loading") { LoadingState(Modifier.height(240.dp), "Loading the releases…") }
            ui.all == 0 -> item(key = "empty") {
                EmptyState(
                    "Nothing is downloading or seeding",
                    Modifier.height(240.dp),
                    body = "Releases appear here once a title is added.",
                )
            }
            else -> {
                item(key = "totals") {
                    Column(Modifier.padding(top = IrisSpace.s5), verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
                        Text(ui.totals, style = IrisType.meta, color = IrisColor.inkMuted)
                        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5), verticalAlignment = Alignment.CenterVertically) {
                            Pill(
                                text = if (state.releaseQuery.isBlank()) "Find a release" else "Finding “${state.releaseQuery.trim()}”",
                                selected = state.releaseQuery.isNotBlank(),
                                onClick = onFind,
                                modifier = Modifier.focusRequester(keys.of(FIND_KEY)),
                            )
                            Text(ui.countWords, style = IrisType.meta, color = IrisColor.inkMuted)
                            if (state.releaseQuery.isNotBlank()) Pill("Clear the search", selected = false, onClick = { actions.onReleaseQuery("") })
                            downloads.errorOrNull?.let { StaleNotice(it) }
                        }
                    }
                }
                if (ui.groups.isEmpty()) {
                    item(key = "no-match") {
                        EmptyState("No release matches “${state.releaseQuery.trim()}”", Modifier.height(180.dp))
                    }
                }
                ui.groups.forEach { g ->
                    item(key = "group:${g.group}") {
                        SectionTitle(g.group.title, meta = g.fact, modifier = Modifier.padding(top = IrisSpace.s7))
                    }
                    items(g.rows, key = { it.infohash }) { row ->
                        ReleaseItem(
                            row,
                            actions.onRelease,
                            state.busy,
                            Modifier.focusRequester(keys.of(row.infohash)),
                            actionsBeside = layout.width >= 840.dp,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun LibraryHints(state: LibraryUiState, focusedIndex: MutableIntState, modifier: Modifier = Modifier) {
    val layout = IrisLayout.current
    val titles = state.titles.valueOrNull
    val hints = buildList {
        add(KeyHint(Keys.OK, if (state.view == LibraryView.Titles) "Open" else "Choose"))
        if (state.view == LibraryView.Titles && titles?.cards?.any { it.ghost } == true) {
            add(KeyHint(Keys.HOLD_OK, "Hide a title no longer on disk"))
        }
        add(KeyHint(Keys.BACK, "To the top, then the menu"))
    }
    val columns = layout.columns(IrisSize.posterGridMin, IrisSpace.s6)
    val trailing = if (state.view == LibraryView.Titles && titles != null && titles.cards.isNotEmpty()) {
        val rows = (titles.cards.size + columns - 1) / columns
        val at = focusedIndex.intValue.coerceAtLeast(0) / columns + 1
        "Row ${at.coerceAtMost(rows)} of $rows"
    } else {
        null
    }
    KeyHints(hints, modifier, trailing = trailing, framed = true)
}

@Composable
internal fun FindAndSortPanel(
    state: LibraryUiState,
    focusSort: Boolean,
    onFilters: (TitleFilters) -> Unit,
    onDismiss: () -> Unit,
) {
    val first = remember { FocusRequester() }
    val sortFocus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { (if (focusSort) sortFocus else first).requestFocus() } }
    val f = state.filters
    SidePanel(title = "Find and sort", onDismiss = onDismiss, footer = "Back closes this panel") {
        PanelLabel("Find a title")
        TextInput(
            value = f.query,
            onValueChange = { onFilters(f.copy(query = it)) },
            label = "Title or TMDB id",
            leadingIcon = Icons.Rounded.Search,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { onDismiss() }),
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = IrisSpace.s4)
                .focusRequester(first),
        )
        PanelLabel("Sort")
        ChosenPanelOptions(
            options = Sort.entries,
            selected = f.sort,
            onSelect = { onFilters(f.copy(sort = it)) },
            label = { it.label },
            selectedFocus = sortFocus,
        )
    }
}

@Composable
private fun FindReleasePanel(query: String, onQuery: (String) -> Unit, onDismiss: () -> Unit) {
    val field = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { field.requestFocus() } }
    SidePanel(title = "Find a release", onDismiss = onDismiss, footer = "By title, release name, hash or who added it") {
        TextInput(
            value = query,
            onValueChange = onQuery,
            label = "Find a release",
            leadingIcon = Icons.Rounded.Search,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { onDismiss() }),
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = IrisSpace.s4)
                .focusRequester(field),
        )
    }
}
