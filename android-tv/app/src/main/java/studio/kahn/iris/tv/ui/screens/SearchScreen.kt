package studio.kahn.iris.tv.ui.screens

import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.speech.RecognizerIntent
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Mic
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material.icons.rounded.Search
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import java.time.ZonedDateTime
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.LibraryMatch
import studio.kahn.iris.tv.data.RecentSearchView
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.SearchViewMode
import studio.kahn.iris.tv.data.TitleCard
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.CardRow
import studio.kahn.iris.tv.ui.components.EmptyState
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.FocusColors
import studio.kahn.iris.tv.ui.components.FocusSurface
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.PillChoice
import studio.kahn.iris.tv.ui.components.PosterCard
import studio.kahn.iris.tv.ui.components.RowCard
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.TextInput
import studio.kahn.iris.tv.ui.components.PosterGrid
import studio.kahn.iris.tv.ui.screens.search.AudioOption
import studio.kahn.iris.tv.ui.screens.search.GrabAsk
import studio.kahn.iris.tv.ui.screens.search.GrabRefusal
import studio.kahn.iris.tv.ui.screens.search.GrabUi
import studio.kahn.iris.tv.ui.screens.search.LibraryMatchRow
import studio.kahn.iris.tv.ui.screens.search.LoadMoreAtEnd
import studio.kahn.iris.tv.ui.screens.search.MatchTarget
import studio.kahn.iris.tv.ui.screens.search.MoreFooter
import studio.kahn.iris.tv.ui.screens.search.ReleaseCard
import studio.kahn.iris.tv.ui.screens.search.ReleaseList
import studio.kahn.iris.tv.ui.screens.search.SearchKeyboard
import studio.kahn.iris.tv.ui.screens.search.KEYBOARD_WIDTH
import studio.kahn.iris.tv.ui.screens.search.SearchKind
import studio.kahn.iris.tv.ui.screens.search.SearchSort
import studio.kahn.iris.tv.ui.screens.search.SearchUiState
import studio.kahn.iris.tv.ui.screens.search.SearchViewModel
import studio.kahn.iris.tv.ui.screens.search.SummaryLine
import studio.kahn.iris.tv.ui.screens.search.failedTrackers
import studio.kahn.iris.tv.ui.screens.search.label
import studio.kahn.iris.tv.ui.screens.search.languageLabel
import studio.kahn.iris.tv.ui.screens.search.pageWords
import studio.kahn.iris.tv.ui.screens.search.parsedWords
import studio.kahn.iris.tv.ui.screens.search.plural
import studio.kahn.iris.tv.ui.screens.search.recentWhen
import studio.kahn.iris.tv.ui.screens.search.releaseKey
import studio.kahn.iris.tv.ui.screens.search.releaseRows
import studio.kahn.iris.tv.ui.screens.search.rememberedFocus
import studio.kahn.iris.tv.ui.screens.search.titleMeta
import studio.kahn.iris.tv.ui.screens.search.titleStatus
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** What the search screen can ask for; the defaults do nothing (screenshots). */
@Immutable
data class SearchActions(
    val onType: (String) -> Unit = {},
    val onKey: (String) -> Unit = {},
    val onDelete: () -> Unit = {},
    val onClear: () -> Unit = {},
    val onSubmit: () -> Unit = {},
    val onEdit: () -> Unit = {},
    val onView: (SearchViewMode) -> Unit = {},
    val onKind: (SearchKind) -> Unit = {},
    val onSort: (SearchSort) -> Unit = {},
    val onLanguage: (String?) -> Unit = {},
    val onRetry: () -> Unit = {},
    val onRetryTitles: () -> Unit = {},
    val onRetryRecent: () -> Unit = {},
    val onLoadMore: () -> Unit = {},
    val onRetryMore: () -> Unit = {},
    val onRecent: (String) -> Unit = {},
    val onForget: (String?) -> Unit = {},
    val onTitle: (TitleCard) -> Unit = {},
    val onRelease: (SearchResult) -> Unit = {},
    val onGrab: (SearchResult) -> Unit = {},
    val onMatch: (MatchTarget) -> Unit = {},
    val onVoice: (() -> Unit)? = null,
    val onConfirmGrab: () -> Unit = {},
    val onDismissGrab: () -> Unit = {},
)

/**
 * Search, from the field to a release (TVSearchStart, TVSearch,
 * TVSearchGrid, TVSearchList). The field comes first; on a TV the
 * on-screen keyboard types into it, OK on the field opens the system
 * keyboard, the remote's search key brings the field back.
 */
@Composable
fun SearchScreen(
    container: AppContainer,
    initialQuery: String?,
    autoPlay: Boolean,
    onOpenRelease: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    onOpenTitle: (query: String, tmdbId: Long, kind: SearchKind, sort: SearchSort) -> Unit,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onOpenCollection: (collectionId: String) -> Unit,
) {
    val vm = irisViewModel(container) { c, _ -> SearchViewModel(c, initialQuery, autoPlay) }
    val state by vm.state.collectAsStateWithLifecycle()
    val grab by vm.grabber.state.collectAsStateWithLifecycle()
    val play by vm.grabber.play.collectAsStateWithLifecycle()
    LaunchedEffect(play) {
        play?.let {
            vm.grabber.played()
            onPlay(it.infohash, it.fileIdx)
        }
    }

    val context = LocalContext.current
    val onScreenKeyboard = remember { !context.packageManager.hasSystemFeature(PackageManager.FEATURE_TOUCHSCREEN) }
    val voiceIntent = remember {
        Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
            .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            .putExtra(RecognizerIntent.EXTRA_PROMPT, "Search Iris")
    }
    val voiceAvailable = remember { voiceIntent.resolveActivity(context.packageManager) != null }
    val voice = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val spoken = result.data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)?.firstOrNull()
        if (result.resultCode == Activity.RESULT_OK && !spoken.isNullOrBlank()) vm.submit(spoken)
    }

    val actions = remember(vm) {
        SearchActions(
            onType = vm::type,
            onKey = vm::key,
            onDelete = vm::backspace,
            onClear = vm::clear,
            onSubmit = { vm.submit() },
            onEdit = vm::edit,
            onView = vm::setView,
            onKind = vm::setKind,
            onSort = vm::setSort,
            onLanguage = vm::setLanguage,
            onRetry = vm::retry,
            onRetryTitles = vm::retryTitles,
            onRetryRecent = vm::loadRecent,
            onLoadMore = vm::loadMore,
            onRetryMore = vm::retryMore,
            onRecent = { vm.submit(it) },
            onForget = vm::forget,
            onTitle = { t ->
                vm.pickedTitle()
                val s = vm.state.value
                onOpenTitle(s.typed.trim().ifEmpty { s.query }, t.tmdbId, s.kind, s.sort)
            },
            onRelease = { r -> onOpenRelease(r.providerId, r.externalId, r.titleMatch?.tmdbId ?: r.tmdbId, (r.titleMatch?.kind ?: r.kind)?.value) },
            onGrab = vm::grab,
            onMatch = { target ->
                when (target) {
                    is MatchTarget.Play -> onPlay(target.infohash, target.fileIdx)
                    is MatchTarget.Open -> onOpenCollection(target.collectionId)
                }
            },
            onConfirmGrab = vm.grabber::confirm,
            onDismissGrab = vm.grabber::dismiss,
        )
    }
    SearchContent(
        state = state,
        grab = grab,
        onScreenKeyboard = onScreenKeyboard,
        actions = if (voiceAvailable) actions.copy(onVoice = { runCatching { voice.launch(voiceIntent) } }) else actions,
    )
}

/** The stateless search screen: what the screenshot tests render. */
@Composable
fun SearchContent(
    state: SearchUiState,
    grab: GrabUi,
    onScreenKeyboard: Boolean,
    actions: SearchActions,
    now: ZonedDateTime = remember { ZonedDateTime.now() },
    /** The result (or title) to focus first instead of the field, by its key. */
    initialFocus: String? = null,
) {
    val layout = IrisLayout.current
    val field = remember { FocusRequester() }
    val restore = remember { FocusRequester() }
    var restoreKey by rememberSaveable { mutableStateOf(initialFocus) }
    var fieldFocused by remember { mutableStateOf(false) }
    val results = state.showsResults

    // Coming back from a release lands on the result left; else the field.
    LaunchedEffect(results) {
        withFrameNanos { }
        val restored = restoreKey != null && runCatching { restore.requestFocus() }.isSuccess
        if (!restored) runCatching { field.requestFocus() }
    }
    // Back comes to the field first; from the field the shell takes it (to the header).
    BackHandler(enabled = !fieldFocused) { runCatching { field.requestFocus() } }
    val onFocused: (String) -> Unit = { restoreKey = it }

    Box(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground)
            .onPreviewKeyEvent { e ->
                if (e.key == Key.Search && e.type == KeyEventType.KeyUp) {
                    actions.onEdit()
                    runCatching { field.requestFocus() }
                    true
                } else {
                    false
                }
            },
    ) {
        Column(
            Modifier
                .fillMaxSize()
                .padding(horizontal = layout.safeHorizontal),
        ) {
            val fieldFocus = FieldFocus(field) { fieldFocused = it }
            if (results) {
                ResultsLayout(state, grab, actions, fieldFocus, restoreKey, restore, onFocused, Modifier.weight(1f))
            } else {
                ComposeLayout(state, onScreenKeyboard, actions, fieldFocus, restoreKey, restore, onFocused, now, Modifier.weight(1f))
            }
        }
        KeyHints(
            hints = hints(state, results),
            trailing = if (results) resultsTrailing(state) else null,
            framed = results,
            modifier = Modifier
                .align(Alignment.BottomStart)
                .then(if (results) Modifier else Modifier.padding(start = layout.safeHorizontal, end = layout.safeHorizontal, bottom = 15.dp)),
        )
        GrabAsk(grab, onConfirm = actions.onConfirmGrab, onCancel = actions.onDismissGrab)
    }
}

/** The search field's focus, wherever the layout puts the field. */
@Immutable
private class FieldFocus(val requester: FocusRequester, val onFocused: (Boolean) -> Unit)

private fun Modifier.fieldFocus(f: FieldFocus): Modifier =
    focusRequester(f.requester).onFocusChanged { f.onFocused(it.isFocused) }

private fun hints(state: SearchUiState, results: Boolean): List<KeyHint> = when {
    results -> listOf(
        KeyHint(Keys.OK, "See the release"),
        KeyHint(Keys.HOLD_OK, "Download and play"),
        KeyHint(Keys.BACK, "To the search field"),
    )
    state.typed.isBlank() && !state.recent.valueOrNull.isNullOrEmpty() -> listOf(
        KeyHint(Keys.OK, "Search again"),
        KeyHint(Keys.HOLD_OK, "Remove from recent"),
        KeyHint(Keys.LEFT, "To the keyboard"),
    )
    else -> listOf(
        KeyHint(Keys.OK, "Type the letter"),
        KeyHint(Keys.RIGHT, "To the results"),
        KeyHint(Keys.BACK, "To the field, then leave"),
    )
}

private fun resultsTrailing(state: SearchUiState): String? {
    val page = state.results?.valueOrNull ?: return null
    val words = pageWords(page.pages, page.providers)
    return if (page.next != null) "$words · next page at the end of the ${if (state.view == SearchViewMode.GRID) "grid" else "list"}" else words
}

@Composable
private fun SearchField(
    state: SearchUiState,
    actions: SearchActions,
    modifier: Modifier,
) {
    val keyboard = LocalSoftwareKeyboardController.current
    TextInput(
        value = state.typed,
        onValueChange = actions.onType,
        label = "Title, year or release name",
        leadingIcon = Icons.Rounded.Search,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search, autoCorrectEnabled = false, showKeyboardOnFocus = false),
        keyboardActions = KeyboardActions(onSearch = {
            keyboard?.hide()
            actions.onSubmit()
        }),
        modifier = modifier
            .fillMaxWidth()
            .onPreviewKeyEvent { e ->
                // OK on the field opens the system keyboard (focus alone does not).
                val ok = e.key == Key.DirectionCenter || e.key == Key.Enter || e.key == Key.NumPadEnter
                if (ok && e.type == KeyEventType.KeyUp) keyboard?.show()
                false
            },
    )
}

@Composable
private fun ComposeLayout(
    state: SearchUiState,
    onScreenKeyboard: Boolean,
    actions: SearchActions,
    fieldFocus: FieldFocus,
    restoreKey: String?,
    restore: FocusRequester,
    onFocused: (String) -> Unit,
    now: ZonedDateTime,
    modifier: Modifier,
) {
    val layout = IrisLayout.current
    Row(
        modifier
            .fillMaxWidth()
            .padding(bottom = 46.dp),
    ) {
        Column(
            Modifier.width(if (onScreenKeyboard) KEYBOARD_WIDTH + 12.dp else minOf(300.dp, layout.contentWidth * 0.36f)),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        ) {
            SearchField(state, actions, Modifier.fieldFocus(fieldFocus))
            if (state.tooShort) StatusLine("Type at least 2 characters.", tone = StatusTone.Warn)
            if (onScreenKeyboard) {
                SearchKeyboard(onType = actions.onKey, onDelete = actions.onDelete, onClear = actions.onClear)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                ActionButton(
                    "Search the trackers",
                    actions.onSubmit,
                    icon = Icons.Rounded.Search,
                    enabled = state.typed.isNotBlank(),
                    size = ActionSize.Small,
                )
                actions.onVoice?.let { voice ->
                    ActionButton("Speak", voice, icon = Icons.Rounded.Mic, style = ActionStyle.Secondary, size = ActionSize.Small)
                }
            }
        }
        Spacer(Modifier.width(IrisSpace.s10))
        Column(
            Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .padding(4.dp),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s6),
        ) {
            if (state.typed.isBlank()) {
                RecentSearches(state, actions, now)
            } else {
                TitlesPanel(state, actions, restoreKey, restore, onFocused)
            }
        }
    }
}

@Composable
private fun RecentSearches(state: SearchUiState, actions: SearchActions, now: ZonedDateTime) {
    val recent = state.recent
    val shown = recent.valueOrNull.orEmpty().take(RECENT_SHOWN)
    when {
        recent is Loadable.Loading -> LoadingState(Modifier.height(160.dp), "Loading your recent searches…")
        recent is Loadable.Failed -> ErrorState(
            recent.error.message,
            actions.onRetryRecent,
            Modifier.height(200.dp),
            title = "Couldn't load your recent searches",
        )
        shown.isEmpty() -> EmptyState(
            "Search for a movie or a series",
            Modifier.height(200.dp),
            body = "Type a title, a year or a release name. Picking a title gives the trackers its exact name.",
        )
        else -> Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
            SectionTitle("Recent searches", meta = "Kept for your account · the last $RECENT_SHOWN")
            recent.errorOrNull?.let { StaleNotice(it) }
            state.forgetError?.let { StatusLine("Not forgotten: ${it.message}", tone = StatusTone.Down) }
            shown.forEach { item -> RecentRow(item, state.forgetting, actions, now) }
            ActionButton(
                "Clear recent searches",
                { actions.onForget(null) },
                style = ActionStyle.Secondary,
                size = ActionSize.Small,
                busy = state.forgetting == "",
                busyText = "Clearing…",
                modifier = Modifier.padding(top = IrisSpace.s1),
            )
        }
    }
}

@Composable
private fun RecentRow(item: RecentSearchView, forgetting: String?, actions: SearchActions, now: ZonedDateTime) {
    RowCard(
        onClick = { actions.onRecent(item.query) },
        onLongClick = { actions.onForget(item.query) },
        modifier = Modifier.heightIn(min = 38.dp),
    ) { _ ->
        val muted = IrisColor.inkMuted
        if (forgetting == item.query) {
            Spinner(Modifier.size(IrisSize.icon), color = muted)
        } else {
            Icon(Icons.Rounded.Schedule, contentDescription = null, tint = muted, modifier = Modifier.size(IrisSize.icon))
        }
        Text(
            item.query,
            style = IrisType.control.copy(fontSize = 14.sp, lineHeight = 17.sp),
            color = IrisColor.ink,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        Text(
            if (forgetting == item.query) "Forgetting…" else recentWhen(item.searchedAt, now),
            style = IrisType.meta,
            color = muted,
        )
    }
}

@Composable
private fun ViewPills(state: SearchUiState, actions: SearchActions) {
    Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3), verticalAlignment = Alignment.CenterVertically) {
        Text("View", style = IrisType.meta, color = IrisColor.inkMuted)
        PillChoice(
            options = SearchViewMode.entries,
            selected = state.view,
            onSelect = actions.onView,
            label = { it.label },
        )
    }
}

@Composable
private fun KindPills(state: SearchUiState, actions: SearchActions) {
    Text("Type", style = IrisType.meta, color = IrisColor.inkMuted)
    PillChoice(SearchKind.entries, state.kind, actions.onKind, label = { it.label })
}

@Composable
private fun TitlesPanel(
    state: SearchUiState,
    actions: SearchActions,
    restoreKey: String?,
    restore: FocusRequester,
    onFocused: (String) -> Unit,
) {
    Row(
        Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ViewPills(state, actions)
        Divider()
        KindPills(state, actions)
    }
    val ownResults = state.typed.trim() == state.query
    if (ownResults) parsedWords(state.results?.valueOrNull?.parsed)?.let { Text(it, style = IrisType.meta, color = IrisColor.inkMuted) }
    if (state.matches.isNotEmpty()) {
        Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
            SectionTitle("In your library")
            state.matches.forEach { m -> LibraryMatchRow(m, actions.onMatch) }
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
        SectionTitle("Titles", meta = "Pick one to see its releases on the trackers")
        val titles = state.titles
        when {
            titles == null || titles is Loadable.Loading -> LoadingState(Modifier.height(150.dp), "Looking for titles…")
            titles is Loadable.Failed -> ErrorState(
                titles.error.message,
                actions.onRetryTitles,
                Modifier.height(170.dp),
                title = "Couldn't look up titles",
            )
            state.titleCards.isEmpty() -> EmptyState(
                "TMDB knows no title for these words",
                Modifier.height(150.dp),
                body = "Grid and List show every release the trackers find.",
            )
            else -> {
                titles.errorOrNull?.let { StaleNotice(it) }
                TitleRow(state.titleCards, state.counts, actions, restoreKey, restore, onFocused)
            }
        }
        val unmatched = if (ownResults) state.shown.count { it.titleMatch == null } else 0
        if (unmatched > 0) {
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "${plural(unmatched, "release")} match no title.",
                    style = IrisType.meta,
                    color = IrisColor.inkMuted,
                )
                ActionButton(
                    "See every release in the list",
                    { actions.onView(SearchViewMode.LIST) },
                    style = ActionStyle.Secondary,
                    size = ActionSize.Small,
                )
            }
        }
    }
    val recent = state.recent.valueOrNull.orEmpty().take(3)
    if (recent.isNotEmpty()) {
        Row(
            Modifier.horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Recent", style = IrisType.meta, color = IrisColor.inkMuted)
            recent.forEach { r ->
                ActionButton(r.query, { actions.onRecent(r.query) }, style = ActionStyle.Secondary, size = ActionSize.Small)
            }
        }
    }
}

@Composable
private fun TitleRow(
    titles: List<TitleCard>,
    counts: Map<Long, Int>,
    actions: SearchActions,
    restoreKey: String?,
    restore: FocusRequester,
    onFocused: (String) -> Unit,
) {
    CardRow(
        items = titles,
        key = ::titleKey,
        contentPadding = PaddingValues(start = 4.dp, end = 4.dp, top = 4.dp, bottom = 8.dp),
        gap = IrisSpace.s6,
    ) { t ->
        val status = titleStatus(t, counts)
        PosterCard(
            title = t.title,
            imageUrl = t.posterUrl,
            onClick = { actions.onTitle(t) },
            kind = titleMeta(t),
            meta = if (status == null) titleMeta(t).ifEmpty { null } else null,
            status = status,
            statusTone = if (t.collectionId != null) StatusTone.Ok else StatusTone.Muted,
            modifier = Modifier.rememberedFocus(titleKey(t), restoreKey, restore, onFocused),
        )
    }
}

private fun titleKey(t: TitleCard) = "${t.kind.value}-${t.tmdbId}"

@Composable
private fun ResultsLayout(
    state: SearchUiState,
    grab: GrabUi,
    actions: SearchActions,
    fieldFocus: FieldFocus,
    restoreKey: String?,
    restore: FocusRequester,
    onFocused: (String) -> Unit,
    modifier: Modifier,
) {
    // A landscape phone has no height for two rows of filters: they share one, scrolled.
    val oneRow = IrisLayout.current.height < 450.dp
    val filters: @Composable () -> Unit = {
        KindPills(state, actions)
        if (state.rows.isNotEmpty() || state.language != null) {
            Divider()
            Text("Audio", style = IrisType.meta, color = IrisColor.inkMuted)
            val selected = state.audio.firstOrNull { it.tag == state.language } ?: state.audio.first()
            PillChoice(state.audio, selected, { o: AudioOption -> actions.onLanguage(o.tag) }, label = { it.words })
        }
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
        Row(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(4.dp),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            CompactField(state.query, actions.onEdit, Modifier.fieldFocus(fieldFocus))
            Spacer(Modifier.width(IrisSpace.s3))
            ViewPills(state, actions)
            Divider()
            PillChoice(SearchSort.entries, state.sort, actions.onSort, label = { it.label })
            if (oneRow) {
                Divider()
                filters()
            }
        }
        if (!oneRow) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState())
                    .padding(horizontal = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
                verticalAlignment = Alignment.CenterVertically,
            ) { filters() }
        }
        val line = listOfNotNull(parsedWords(state.results?.valueOrNull?.parsed), state.summaryLine).joinToString(" ")
        SummaryLine(line, failedTrackers(state.results?.valueOrNull?.providers.orEmpty()), actions.onRetry, Modifier.padding(horizontal = 4.dp))
        GrabRefusal(grab, actions.onDismissGrab)
        ResultsBody(state, grab, actions, restoreKey, restore, onFocused)
    }
}

@Composable
private fun Divider() {
    Box(
        Modifier
            .padding(horizontal = IrisSpace.s1)
            .width(1.dp)
            .height(18.dp)
            .background(IrisColor.line),
    )
}

/** The query in the results bar: OK brings back the field and the keyboard. */
@Composable
private fun CompactField(query: String, onEdit: () -> Unit, modifier: Modifier) {
    FocusSurface(
        onClick = onEdit,
        modifier = modifier
            .width(220.dp)
            .heightIn(min = 30.dp),
        shape = IrisShape.card,
        colors = FocusColors.Row,
        contentAlignment = Alignment.CenterStart,
    ) {
        Row(
            Modifier.padding(horizontal = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Rounded.Search, contentDescription = null, tint = IrisColor.inkMuted, modifier = Modifier.size(13.dp))
            Text(query, style = IrisType.control.copy(fontSize = 13.sp), color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun ResultsBody(
    state: SearchUiState,
    grab: GrabUi,
    actions: SearchActions,
    restoreKey: String?,
    restore: FocusRequester,
    onFocused: (String) -> Unit,
) {
    val bottom = 46.dp + IrisSpace.s3
    when (val results = state.results) {
        null, Loadable.Loading -> LoadingState(Modifier.padding(bottom = bottom), "Asking the trackers…")
        is Loadable.Failed -> ErrorState(results.error.message, actions.onRetry, Modifier.padding(bottom = bottom), title = "The search failed")
        else -> {
            val page = results.valueOrNull ?: return
            val shown = state.shown
            val busyKey = (grab as? GrabUi.Busy)?.key
            if (shown.isEmpty() && state.matches.isEmpty()) {
                val language = languageLabel(state.language, long = false)
                if (language != null && state.rows.isNotEmpty()) {
                    EmptyState(
                        "No release in $language among the ${state.rows.size} loaded",
                        Modifier.padding(bottom = bottom),
                        body = "The audio filter only sees the loaded releases: choose another language.",
                    )
                } else {
                    EmptyState(
                        "Nothing found for “${state.query}”",
                        Modifier.padding(bottom = bottom),
                        body = "Try another spelling, drop the year, or choose Movies or Series.",
                    )
                }
                return
            }
            val hasNext = page.next != null
            if (state.view == SearchViewMode.GRID) {
                val grid = rememberLazyGridState()
                LoadMoreAtEnd(grid, enabled = hasNext && !state.loadingMore && state.moreError == null, onLoadMore = actions.onLoadMore)
                Box(Modifier.fillMaxSize()) {
                    PosterGrid(
                        items = shown,
                        key = ::releaseKey,
                        state = grid,
                        minCell = IrisSize.posterDenseMin,
                        contentPadding = PaddingValues(start = 4.dp, end = 4.dp, top = 6.dp, bottom = bottom),
                        horizontalGap = 14.dp,
                        verticalGap = IrisSpace.s6,
                        header = {
                            state.matches.forEach { m ->
                                item(key = "match-${m.collectionId}", span = { GridItemSpan(maxLineSpan) }) {
                                    LibraryMatchRow(m, actions.onMatch)
                                }
                            }
                        },
                    ) { r ->
                        val key = releaseKey(r)
                        ReleaseCard(
                            r = r,
                            onClick = { actions.onRelease(r) },
                            onLongClick = { actions.onGrab(r) },
                            busy = busyKey == key,
                            modifier = Modifier.rememberedFocus(key, restoreKey, restore, onFocused),
                        )
                    }
                    if (state.loadingMore || state.moreError != null) {
                        MoreFooter(
                            state.loadingMore,
                            state.moreError,
                            hasNext,
                            actions.onRetryMore,
                            Modifier
                                .align(Alignment.BottomCenter)
                                .padding(bottom = 46.dp)
                                .background(IrisColor.ground),
                        )
                    }
                }
            } else {
                val list = rememberLazyListState()
                LoadMoreAtEnd(list, enabled = hasNext && !state.loadingMore && state.moreError == null, onLoadMore = actions.onLoadMore)
                ReleaseList(list, contentPadding = PaddingValues(start = 4.dp, end = 4.dp, top = 4.dp, bottom = bottom)) {
                    items(state.matches, key = { "match-${it.collectionId}" }) { m: LibraryMatch -> LibraryMatchRow(m, actions.onMatch) }
                    releaseRows(shown, grab, restoreKey, restore, onFocused, actions.onRelease, actions.onGrab)
                    item(key = "more") { MoreFooter(state.loadingMore, state.moreError, hasNext, actions.onRetryMore) }
                }
            }
        }
    }
}

private const val RECENT_SHOWN = 5
