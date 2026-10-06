package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.List
import androidx.compose.material.icons.rounded.BookmarkAdd
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.VideoFile
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
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Artwork
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.ChipTone
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.FactRow
import studio.kahn.iris.tv.ui.components.FramedBlock
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.KeyHints
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.PanelOptions
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.focusRing
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.formatSize
import studio.kahn.iris.tv.ui.screens.search.FollowState
import studio.kahn.iris.tv.ui.screens.search.GrabAsk
import studio.kahn.iris.tv.ui.screens.search.GrabRefusal
import studio.kahn.iris.tv.ui.screens.search.GrabUi
import studio.kahn.iris.tv.ui.screens.search.ReleaseSheet
import studio.kahn.iris.tv.ui.screens.search.ReleaseUiState
import studio.kahn.iris.tv.ui.screens.search.ReleaseViewModel
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisFocus
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** What the release page can ask for; the defaults do nothing (screenshots). */
@Immutable
data class ReleaseActions(
    val onGrab: () -> Unit = {},
    val onPlayOwned: () -> Unit = {},
    val onRetry: () -> Unit = {},
    val onPickFile: (Int) -> Unit = {},
    val onFollow: () -> Unit = {},
    val onOtherReleases: ((title: String, tmdbId: Long) -> Unit)? = null,
    val onConfirmGrab: () -> Unit = {},
    val onDismissGrab: () -> Unit = {},
)

/**
 * A release before grabbing it (TVRelease): its title and part, the main
 * action (download and play, or play from disk), following the series,
 * the swarm and the technical sheet as facts, the file to play, and the
 * tracker's own notes and NFO, readable in full in a side panel.
 */
@Composable
fun SearchDetailScreen(
    container: AppContainer,
    providerId: String,
    externalId: String,
    tmdbId: Long?,
    kind: String?,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onOtherReleases: (title: String, tmdbId: Long) -> Unit,
) {
    val vm = irisViewModel(container) { c, _ -> ReleaseViewModel(c, providerId, externalId, tmdbId, kind) }
    val state by vm.state.collectAsStateWithLifecycle()
    val grab by vm.grabber.state.collectAsStateWithLifecycle()
    val play by vm.grabber.play.collectAsStateWithLifecycle()
    LaunchedEffect(play) {
        play?.let {
            vm.grabber.played()
            onPlay(it.infohash, it.fileIdx)
        }
    }
    val actions = remember(vm) {
        ReleaseActions(
            onGrab = vm::grab,
            onPlayOwned = vm::playOwned,
            onRetry = vm::loadPreview,
            onPickFile = vm::pickFile,
            onFollow = vm::follow,
            onOtherReleases = onOtherReleases,
            onConfirmGrab = vm.grabber::confirm,
            onDismissGrab = vm.grabber::dismiss,
        )
    }
    ReleaseContent(state, grab, actions)
}

/** A side panel of the release page: the notes or the NFO in full, the file to play. */
enum class ReleasePanel { Notes, Nfo, Files }

@Composable
fun ReleaseContent(state: ReleaseUiState, grab: GrabUi, actions: ReleaseActions, initialPanel: ReleasePanel? = null) {
    val layout = IrisLayout.current
    val sheet = remember(state) { state.sheet }
    val narrow = layout.width < 900.dp
    var panel by rememberSaveable { mutableStateOf(initialPanel) }
    val primary = remember { FocusRequester() }
    val ready = state.preview is Loadable.Ready || state.preview is Loadable.Stale
    LaunchedEffect(ready, panel) {
        if (!ready || panel != null) return@LaunchedEffect
        withFrameNanos { }
        runCatching { primary.requestFocus() }
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
    ) {
        Row(
            Modifier
                .fillMaxSize()
                .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = layout.safeVertical, bottom = HINTS_BAND),
            horizontalArrangement = Arrangement.spacedBy(if (narrow) IrisSpace.s8 else IrisSpace.s9),
        ) {
            Column(Modifier.width(if (narrow) 110.dp else IrisSize.posterAside), verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
                Text("${sheet.title} · releases", style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Artwork(title = sheet.title, imageUrl = sheet.posterUrl, width = if (narrow) 110.dp else IrisSize.posterAside)
                sheet.kindLine?.let { Text(it, style = IrisType.meta, color = IrisColor.inkMuted) }
            }
            Column(
                Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(4.dp),
                verticalArrangement = Arrangement.spacedBy(IrisSpace.s5),
            ) {
                Heading(sheet)
                when (val preview = state.preview) {
                    Loadable.Loading -> LoadingState(Modifier.height(80.dp), "Reading the torrent…")
                    is Loadable.Failed -> ErrorState(
                        preview.error.message,
                        actions.onRetry,
                        Modifier.height(140.dp),
                        title = "Couldn't read this release",
                        retryFocus = primary,
                    )
                    else -> Actions(state, sheet, grab, actions, primary) { panel = it }
                }
                Details(state, sheet, narrow) { panel = it }
            }
        }
        KeyHints(
            hints = listOf(
                KeyHint(Keys.OK, if (sheet.owned != null) "Play from disk" else "Download and play"),
                KeyHint(Keys.DOWN, "Read the details"),
                KeyHint(Keys.BACK, "To the releases"),
            ),
            modifier = Modifier
                .align(Alignment.BottomStart)
                .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, bottom = 15.dp),
        )
        when (panel) {
            ReleasePanel.Notes -> state.notes?.let { notes ->
                SidePanel("Release notes from ${state.providerId}", onDismiss = { panel = null }, footer = "Written by the uploader") {
                    Paragraphs(paragraphsOf(notes), IrisType.reading)
                }
            }
            ReleasePanel.Nfo -> state.details?.nfo?.let { nfo ->
                SidePanel("Technical sheet (NFO)", onDismiss = { panel = null }) {
                    Paragraphs(nfoChunks(nfo), IrisType.mono)
                }
            }
            ReleasePanel.Files -> SidePanel("File to play", onDismiss = { panel = null }) {
                PanelOptions(
                    options = sheet.videos,
                    selected = sheet.videos.firstOrNull { it.index == sheet.chosenFile },
                    onSelect = {
                        actions.onPickFile(it.index)
                        panel = null
                    },
                    label = { "${it.path.substringAfterLast('/')} · ${formatSize(it.sizeBytes)}" },
                )
            }
            null -> Unit
        }
        GrabAsk(grab, onConfirm = actions.onConfirmGrab, onCancel = actions.onDismissGrab)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Heading(sheet: ReleaseSheet) {
    Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
        Text(sheet.heading, style = IrisType.title, color = IrisColor.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
        if (sheet.name.isNotEmpty()) Text(sheet.name, style = IrisType.mono, color = IrisColor.inkMuted)
        if (sheet.chips.isNotEmpty()) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                sheet.chips.forEach { c -> Chip(c, tone = if (c == "Freeleech") ChipTone.Ok else ChipTone.Neutral) }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Actions(
    state: ReleaseUiState,
    sheet: ReleaseSheet,
    grab: GrabUi,
    actions: ReleaseActions,
    primary: FocusRequester,
    onPanel: (ReleasePanel) -> Unit,
) {
    val busy = grab is GrabUi.Busy
    Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        if (sheet.owned != null) {
            StatusLine("This release is already in your library: it plays from disk, nothing to download.", tone = StatusTone.Ok)
        }
        sheet.blocked?.let { reason ->
            StatusLine(reason, tone = if (sheet.dead) StatusTone.Warn else StatusTone.Down)
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5), verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
            if (sheet.owned != null) {
                ActionButton("Play from disk", actions.onPlayOwned, icon = Icons.Rounded.PlayArrow, size = ActionSize.Large, modifier = Modifier.focusRequester(primary))
                ActionButton(
                    "Download anyway",
                    actions.onGrab,
                    icon = Icons.Rounded.Download,
                    style = ActionStyle.Secondary,
                    size = ActionSize.Large,
                    enabled = sheet.blocked == null,
                    busy = busy,
                    busyText = "Starting the download…",
                )
            } else {
                ActionButton(
                    sheet.playLabel,
                    actions.onGrab,
                    icon = Icons.Rounded.Download,
                    size = ActionSize.Large,
                    enabled = sheet.blocked == null,
                    busy = busy,
                    busyText = "Starting the download…",
                    modifier = Modifier.focusRequester(primary),
                )
            }
            when (val follow = state.follow) {
                is FollowState.Can -> ActionButton(
                    "Follow the series",
                    actions.onFollow,
                    icon = Icons.Rounded.BookmarkAdd,
                    style = ActionStyle.Secondary,
                    size = ActionSize.Large,
                    busy = follow.busy,
                    busyText = "Following…",
                )
                FollowState.Following, FollowState.Hidden -> Unit
            }
            val other = actions.onOtherReleases
            val tmdbId = sheet.tmdbId
            if (other != null && tmdbId != null) {
                ActionButton(
                    "Other releases",
                    { other(sheet.title, tmdbId) },
                    icon = Icons.AutoMirrored.Rounded.List,
                    style = ActionStyle.Secondary,
                    size = ActionSize.Large,
                )
            }
        }
        val follow = state.follow
        if (follow == FollowState.Following) StatusLine("You follow this series: new episodes show on your home screen.", tone = StatusTone.Ok)
        if (follow is FollowState.Can && follow.error != null) StatusLine("Not followed: ${follow.error.message}", tone = StatusTone.Down)
        if (sheet.owned == null && sheet.blocked == null) {
            Text(
                "Playback starts once the first minutes are on disk. The rest keeps downloading while you watch.",
                style = IrisType.meta,
                color = IrisColor.inkMuted,
            )
        }
        GrabRefusal(grab, actions.onDismissGrab)
    }
}

@Composable
private fun Details(state: ReleaseUiState, sheet: ReleaseSheet, narrow: Boolean, onPanel: (ReleasePanel) -> Unit) {
    val facts: @Composable (Modifier) -> Unit = { modifier ->
        if (sheet.facts.isNotEmpty()) {
            Column(modifier) {
                sheet.facts.forEach { (label, value) -> FactRow(label, value) }
                val chosen = sheet.videos.firstOrNull { it.index == sheet.chosenFile }
                if (sheet.videos.size > 1 && chosen != null) {
                    FactRow("Plays") {
                        Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                            Text(chosen.path.substringAfterLast('/'), style = IrisType.mono, color = IrisColor.ink)
                            ActionButton(
                                "Choose another file",
                                { onPanel(ReleasePanel.Files) },
                                icon = Icons.Rounded.VideoFile,
                                style = ActionStyle.Secondary,
                                size = ActionSize.Small,
                            )
                        }
                    }
                }
            }
        }
    }
    val notes: @Composable (Modifier) -> Unit = { modifier -> Notes(state, modifier, onPanel) }
    if (narrow) {
        facts(Modifier.fillMaxWidth())
        notes(Modifier.fillMaxWidth())
    } else {
        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s8), verticalAlignment = Alignment.Top) {
            facts(Modifier.width(320.dp))
            notes(Modifier.weight(1f))
        }
    }
}

@Composable
private fun Notes(state: ReleaseUiState, modifier: Modifier, onPanel: (ReleasePanel) -> Unit) {
    val notes = state.notes
    val nfo = state.details?.nfo?.takeIf { it.isNotBlank() }
    if (notes == null && nfo == null) return
    FramedBlock(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Release notes from ${state.providerId}", style = IrisType.group, color = IrisColor.ink, modifier = Modifier.weight(1f))
            Text("Written by the uploader", style = IrisType.metaSmall, color = IrisColor.inkMuted)
        }
        if (notes != null) {
            Text(notes, style = IrisType.reading, color = IrisColor.ink, maxLines = 10, overflow = TextOverflow.Ellipsis)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
            if (notes != null) ActionButton("Read all", { onPanel(ReleasePanel.Notes) }, style = ActionStyle.Secondary, size = ActionSize.Small)
            if (nfo != null) ActionButton("Technical sheet (NFO)", { onPanel(ReleasePanel.Nfo) }, style = ActionStyle.Secondary, size = ActionSize.Small)
        }
    }
}

/** Long text in a side panel, one focusable block at a time so the D-pad scrolls it. */
@Composable
private fun Paragraphs(blocks: List<AnnotatedString>, style: androidx.compose.ui.text.TextStyle) {
    val first = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { first.requestFocus() } }
    blocks.forEachIndexed { i, block ->
        var focused by remember { mutableStateOf(false) }
        Text(
            block,
            style = style,
            color = IrisColor.ink,
            modifier = Modifier
                .fillMaxWidth()
                .padding(IrisFocus.ringWidth + IrisFocus.ringOffset)
                .then(if (i == 0) Modifier.focusRequester(first) else Modifier)
                .onFocusChanged { focused = it.isFocused }
                .focusRing(focused, IrisShape.key)
                .focusable()
                .padding(horizontal = 10.dp, vertical = IrisSpace.s1),
        )
        Spacer(Modifier.height(IrisSpace.s1))
    }
}

/** A text's paragraphs (blank-line separated), styles kept. */
fun paragraphsOf(text: AnnotatedString): List<AnnotatedString> {
    val out = mutableListOf<AnnotatedString>()
    var start = 0
    val s = text.text
    while (start < s.length) {
        val end = s.indexOf("\n\n", start).let { if (it < 0) s.length else it }
        if (end > start) out += text.subSequence(start, end)
        start = end + 2
    }
    return out.ifEmpty { listOf(text) }
}

/** An NFO in blocks of a dozen lines (it has no paragraphs to speak of). */
fun nfoChunks(nfo: String): List<AnnotatedString> =
    nfo.replace("\r\n", "\n").trimEnd().lines().chunked(NFO_LINES).map { AnnotatedString(it.joinToString("\n")) }

private const val NFO_LINES = 12

/** The room the key hints take at the bottom (board: content ends 110 px above the edge). */
private val HINTS_BAND = 55.dp
