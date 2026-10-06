package studio.kahn.iris.tv.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.Artwork
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.ChipSize
import studio.kahn.iris.tv.ui.components.EmptyState
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.FooterLayout
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.ScreenFooter
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.RowCard
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.screens.library.HistoryGroupUi
import studio.kahn.iris.tv.ui.screens.library.HistoryLine
import studio.kahn.iris.tv.ui.screens.library.HistoryUiState
import studio.kahn.iris.tv.ui.screens.library.HistoryViewModel
import studio.kahn.iris.tv.ui.screens.library.LineAction
import studio.kahn.iris.tv.ui.screens.library.NoticeLine
import studio.kahn.iris.tv.ui.screens.library.plural
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

@Immutable
data class HistoryActions(
    val onPlay: (infohash: String, fileIdx: Int) -> Unit = { _, _ -> },
    val onRestore: (HistoryLine) -> Unit = {},
    val onOpenTitle: (collectionId: String) -> Unit = {},
    val onRetry: () -> Unit = {},
    val onBack: () -> Unit = {},
)

/**
 * My watch history (web `/history`): a series under its title with each episode watched, a
 * film on one line; how far and when, in words. What is gone from disk says so and, when its
 * release is known, is downloaded again with OK and plays from where it stopped.
 */
@Composable
fun HistoryScreen(
    container: AppContainer,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onOpenCollection: (collectionId: String) -> Unit,
    onBack: () -> Unit,
) {
    val vm = irisViewModel(container) { c, _ -> HistoryViewModel(c) }
    val state by vm.state.collectAsStateWithLifecycle()
    val event by vm.playEvents.collectAsStateWithLifecycle()
    RepeatWhileStarted(Unit) { vm.pollWhileStarted() }
    LaunchedEffect(event) {
        val (infohash, idx) = event ?: return@LaunchedEffect
        vm.consumePlay()
        onPickFile(infohash, idx)
    }
    HistoryContent(
        state = state,
        actions = HistoryActions(
            onPlay = onPickFile,
            onRestore = vm::restore,
            onOpenTitle = onOpenCollection,
            onRetry = vm::retry,
            onBack = onBack,
        ),
    )
}

@Composable
fun HistoryContent(state: HistoryUiState, actions: HistoryActions) {
    val layout = IrisLayout.current
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val first = remember { FocusRequester() }
    var atFirst by remember { mutableStateOf(false) }
    val groups = state.groups.valueOrNull
    BackHandler {
        if (atFirst || groups.isNullOrEmpty()) {
            actions.onBack()
        } else {
            scope.launch {
                list.scrollToItem(0)
                runCatching { first.requestFocus() }
            }
        }
    }
    LaunchedEffect(groups.isNullOrEmpty()) {
        if (groups.isNullOrEmpty()) return@LaunchedEffect
        snapshotFlow { list.layoutInfo.visibleItemsInfo.size > 1 }.first { it }
        runCatching { first.requestFocus() }
    }
    FooterLayout(
        footer = {
            ScreenFooter(listOf(KeyHint(Keys.OK, "Play, or download again"), KeyHint(Keys.BACK, "To the top, then back"))) {
                NoticeLine(state.notice)
            }
        },
        modifier = Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
    ) { footer ->
        LazyColumn(
            state = list,
            modifier = Modifier
                .fillMaxSize()
                .padding(bottom = footer),
            contentPadding = PaddingValues(
                start = layout.safeHorizontal,
                end = layout.safeHorizontal,
                top = layout.safeVertical,
                bottom = IrisSpace.s4,
            ),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            item(key = "heading") {
                Column(Modifier.padding(bottom = IrisSpace.s4), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                    SectionTitle(
                        "Watch history",
                        meta = groups?.takeIf { it.isNotEmpty() }?.let { plural(it.size, "title") } ?: "What you watched, finished or not.",
                        style = IrisType.page,
                    )
                    state.groups.errorOrNull?.takeIf { groups != null }?.let { StaleNotice(it) }
                }
            }
            when {
                state.groups is Loadable.Failed -> item(key = "failed") {
                    ErrorState(state.groups.error.message, actions.onRetry, Modifier.widthIn(max = 600.dp))
                }
                groups == null -> item(key = "loading") { LoadingState(label = "Loading your history…") }
                groups.isEmpty() -> item(key = "empty") {
                    EmptyState("Nothing watched yet.", body = "What you play shows here, with where you stopped.")
                }
                else -> items(groups, key = { it.key }) { group ->
                    HistoryGroupRow(
                        group = group,
                        busy = state.busy,
                        actions = actions,
                        firstFocus = if (group == groups.first()) first else null,
                        onFirstFocused = { atFirst = it },
                    )
                }
            }
        }
    }
}

@Composable
private fun HistoryGroupRow(
    group: HistoryGroupUi,
    busy: Set<String>,
    actions: HistoryActions,
    firstFocus: FocusRequester?,
    onFirstFocused: (Boolean) -> Unit,
) {
    val firstModifier = if (firstFocus != null) {
        Modifier
            .focusRequester(firstFocus)
            .onFocusChanged { onFirstFocused(it.hasFocus) }
    } else {
        Modifier
    }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = IrisSpace.s3),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
    ) {
        Artwork(group.title, group.posterUrl, width = IrisSize.posterMini, showTitle = false)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            if (group.solo) {
                LineCard(group, group.lines.first(), busy, actions, firstModifier)
            } else {
                val open = group.collectionId
                RowCard(
                    onClick = { open?.let(actions.onOpenTitle) },
                    enabled = open != null,
                    modifier = firstModifier,
                ) { _ ->
                    Text(group.title, style = IrisType.group, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                    if (group.ghost) Chip("Gone from disk", size = ChipSize.Small)
                    if (open != null) Text("Open the title", style = IrisType.controlSmall, color = IrisColor.accent)
                }
                group.lines.forEach { line -> LineCard(group, line, busy, actions, Modifier.padding(start = IrisSpace.s6)) }
            }
        }
    }
}

@Composable
private fun LineCard(
    group: HistoryGroupUi,
    line: HistoryLine,
    busy: Set<String>,
    actions: HistoryActions,
    modifier: Modifier = Modifier,
) {
    val restoring = "restore:${line.key}" in busy
    RowCard(
        onClick = {
            when (line.action) {
                LineAction.Play -> actions.onPlay(line.infohash, line.fileIdx)
                LineAction.Restore -> actions.onRestore(line)
                LineAction.OpenTitle -> group.collectionId?.let(actions.onOpenTitle)
                LineAction.None -> Unit
            }
        },
        enabled = line.action != LineAction.None,
        modifier = modifier,
    ) { _ ->
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
            Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3), verticalAlignment = Alignment.CenterVertically) {
                Text(line.label, style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                if (line.deleted) Chip("Gone from disk", size = ChipSize.Small)
            }
            Text(line.facts, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Meter(line.share, Modifier.widthIn(max = 220.dp))
        }
        val verb = when (line.action) {
            LineAction.Play -> "Play"
            LineAction.Restore -> if (restoring) "Asking the server…" else "Download again"
            LineAction.OpenTitle -> "Open the title"
            LineAction.None -> null
        }
        verb?.let { Text(it, style = IrisType.controlSmall, color = IrisColor.accent, maxLines = 1) }
    }
}
