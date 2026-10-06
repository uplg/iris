package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import kotlinx.coroutines.flow.first
import androidx.compose.runtime.snapshotFlow
import androidx.compose.foundation.lazy.rememberLazyListState
import studio.kahn.iris.tv.ui.components.focusReturn
import studio.kahn.iris.tv.ui.components.rememberFocusReturn
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.Artwork
import studio.kahn.iris.tv.ui.components.ConfirmDialog
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.FooterLayout
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.ScreenFooter
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.RowCard
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.screens.library.DetailUiState
import studio.kahn.iris.tv.ui.screens.library.DetailViewModel
import studio.kahn.iris.tv.ui.screens.library.ReleaseActions
import studio.kahn.iris.tv.ui.screens.library.ReleaseItem
import studio.kahn.iris.tv.ui.screens.library.ReleaseRow
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.components.NoticeLine

@Immutable
data class DetailActions(
    val onPlay: (fileIdx: Int) -> Unit = {},
    val onRelease: ReleaseActions = ReleaseActions(),
    val onRetry: () -> Unit = {},
    val onBack: () -> Unit = {},
)

/**
 * One release and its video files (a box set, a season pack, a multi-disc rip): its state in
 * words, pause or resume, delete, and every video with how far you watched it. OK plays.
 */
@Composable
fun DetailScreen(
    container: AppContainer,
    infohash: String,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onOpenCollection: (collectionId: String) -> Unit,
    onBack: () -> Unit,
) {
    val vm = irisViewModel(container, key = "release:$infohash") { c, _ -> DetailViewModel(c, infohash) }
    val state by vm.state.collectAsStateWithLifecycle()
    RepeatWhileStarted(Unit) { vm.pollWhileStarted() }
    LaunchedEffect(state.gone) { if (state.gone) onBack() }
    DetailContent(
        state = state,
        actions = DetailActions(
            onPlay = { onPickFile(infohash, it) },
            onRelease = ReleaseActions(
                onOpenTitle = onOpenCollection,
                onPause = vm::pause,
                onResume = vm::resume,
                onDelete = vm::delete,
            ),
            onRetry = vm::retry,
            onBack = onBack,
        ),
    )
}

@Composable
fun DetailContent(state: DetailUiState, actions: DetailActions) {
    val layout = IrisLayout.current
    val firstFile = remember { FocusRequester() }
    val keys = rememberFocusReturn(fallback = firstFile)
    val list = rememberLazyListState()
    var deleting by remember { mutableStateOf<ReleaseRow?>(null) }
    FooterLayout(
        footer = {
            ScreenFooter(listOf(KeyHint(Keys.OK, "Play"), KeyHint(Keys.BACK, "Back"))) { NoticeLine(state.notice) }
        },
        modifier = Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
    ) { footer ->
        when (val page = state.page) {
            Loadable.Loading -> LoadingState(label = "Loading the release…")
            is Loadable.Failed -> ErrorState(page.error.message, actions.onRetry)
            is Loadable.Ready, is Loadable.Stale -> {
                val p = page.valueOrNull ?: return@FooterLayout
                // Back from the player: the file left (episode 12 of a pack, not episode 1).
                LaunchedEffect(p.files.isNotEmpty()) {
                    if (p.files.isEmpty()) return@LaunchedEffect
                    val left = p.files.indexOfFirst { "file:${it.index}" == keys.last }
                    if (left >= 0) {
                        val item = FILES_FIRST + left
                        if (list.layoutInfo.visibleItemsInfo.none { it.index == item }) list.scrollToItem(item)
                        snapshotFlow { list.layoutInfo.visibleItemsInfo.any { it.index == item } }.first { it }
                        if (keys.focusLast()) return@LaunchedEffect
                    }
                    runCatching { firstFile.requestFocus() }
                }
                val compact = layout.height < 500.dp
                Row(
                    Modifier
                        .fillMaxSize()
                        .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = layout.safeVertical),
                    horizontalArrangement = Arrangement.spacedBy(if (compact) IrisSpace.s7 else IrisSpace.s9),
                ) {
                    Column(Modifier.width(if (compact) 96.dp else IrisSize.posterAside), verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
                        Eyebrow("${p.kind} · release")
                        Artwork(p.title, p.posterUrl, width = if (compact) 96.dp else IrisSize.posterAside, titleStyle = IrisType.group)
                    }
                    LazyColumn(
                        Modifier
                            .weight(1f)
                            .padding(bottom = footer),
                        state = list,
                        contentPadding = PaddingValues(bottom = IrisSpace.s4, start = IrisSpace.s2, end = IrisSpace.s2, top = IrisSpace.s1),
                        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
                    ) {
                        item(key = "title") {
                            Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                                Text(p.title, style = IrisType.title, color = IrisColor.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                page.errorOrNull?.let { StaleNotice(it) }
                            }
                        }
                        item(key = "release") {
                            ReleaseItem(
                                p.row,
                                actions.onRelease.copy(onDelete = { deleting = it }),
                                state.busy,
                                Modifier.focusReturn(keys, RELEASE_KEY),
                                showTitle = false,
                            )
                        }
                        item(key = "files") {
                            SectionTitle("Video files", meta = plural(p.files.size, "file"), modifier = Modifier.padding(top = IrisSpace.s5))
                        }
                        if (p.files.isEmpty()) {
                            item(key = "no-files", contentType = "hint") {
                                Text("No video file in this release.", style = IrisType.meta, color = IrisColor.inkMuted)
                            }
                        }
                        items(p.files, key = { it.index }) { f ->
                            RowCard(
                                onClick = { actions.onPlay(f.index) },
                                modifier = Modifier
                                    .focusReturn(keys, "file:${f.index}")
                                    .then(if (f == p.files.first()) Modifier.focusRequester(firstFile) else Modifier),
                            ) { _ ->
                                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
                                    Text(f.name, style = IrisType.mono, color = IrisColor.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                    if (f.watched) {
                                        StatusLine(f.facts, tone = StatusTone.Ok)
                                    } else {
                                        Text(f.facts, style = IrisType.meta, color = IrisColor.inkMuted)
                                    }
                                }
                                Text(if (f.resume) "Resume" else "Play", style = IrisType.controlSmall, color = IrisColor.accent)
                            }
                        }
                    }
                }
            }
        }
        deleting?.let { row ->
            ConfirmDialog(
                eyebrow = "Delete a release",
                title = "Delete ${row.release}?",
                body = row.deleteBody,
                confirmLabel = "Delete release",
                onConfirm = {
                    deleting = null
                    actions.onRelease.onDelete(row)
                    keys.returnTo(RELEASE_KEY)
                },
                onCancel = {
                    deleting = null
                    keys.returnTo(RELEASE_KEY)
                },
            )
        }
    }
}

/** The list's items before the files: the title, the release, the files' heading. */
private const val FILES_FIRST = 3
private const val RELEASE_KEY = "release"
