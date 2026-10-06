package studio.kahn.iris.tv.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.BookmarkBorder
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Language
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.RemoveDone
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
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Text
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.ui.format.NO_SUBTITLES
import studio.kahn.iris.tv.ui.format.audioChoiceWords
import studio.kahn.iris.tv.ui.format.subtitleChoiceWords
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSheet
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Artwork
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.ChipTone
import studio.kahn.iris.tv.ui.components.ConfirmDialog
import studio.kahn.iris.tv.ui.components.ErrorState
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.FactRow
import studio.kahn.iris.tv.ui.components.FramedBlock
import studio.kahn.iris.tv.ui.components.KeyHint
import studio.kahn.iris.tv.ui.components.FooterLayout
import studio.kahn.iris.tv.ui.components.Keys
import studio.kahn.iris.tv.ui.components.ScreenFooter
import studio.kahn.iris.tv.ui.components.LanguageChip
import studio.kahn.iris.tv.ui.components.LoadingState
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.PanelLabel
import studio.kahn.iris.tv.ui.components.PanelOptions
import studio.kahn.iris.tv.ui.components.PillChoice
import studio.kahn.iris.tv.ui.components.RowCard
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.SidePanel
import studio.kahn.iris.tv.ui.components.StaleNotice
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.screens.library.CollectionPage
import studio.kahn.iris.tv.ui.screens.library.CollectionUiState
import studio.kahn.iris.tv.ui.screens.library.CollectionViewModel
import studio.kahn.iris.tv.ui.screens.library.EpisodeAction
import studio.kahn.iris.tv.ui.screens.library.EpisodeRowUi
import studio.kahn.iris.tv.ui.screens.library.FileUi
import studio.kahn.iris.tv.ui.components.rememberFocusReturn
import studio.kahn.iris.tv.ui.components.focusReturn
import androidx.compose.runtime.withFrameNanos
import studio.kahn.iris.tv.ui.screens.library.GoneUi
import studio.kahn.iris.tv.ui.screens.library.LanguagesUi
import studio.kahn.iris.tv.ui.screens.library.PackUi
import studio.kahn.iris.tv.ui.screens.library.ReleaseActions
import studio.kahn.iris.tv.ui.screens.library.ReleaseItem
import studio.kahn.iris.tv.ui.screens.library.ReleaseRow
import studio.kahn.iris.tv.ui.screens.library.busyKey
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted
import studio.kahn.iris.tv.ui.state.irisViewModel
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.format.languageName
import studio.kahn.iris.tv.ui.format.markWatchedLabel
import studio.kahn.iris.tv.ui.screens.library.WATCHED_KEY
import studio.kahn.iris.tv.ui.format.plural
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.NoticeLine

/** Everything a title's page hands back. */
@Immutable
data class CollectionActions(
    val onPlay: (infohash: String, fileIdx: Int) -> Unit = { _, _ -> },
    val onEpisode: (EpisodeRowUi, EpisodeAction) -> Unit = { _, _ -> },
    val onSeason: (Long) -> Unit = {},
    val onWatchlist: () -> Unit = {},
    val onWatched: () -> Unit = {},
    val onPack: (PackUi, Boolean) -> Unit = { _, _ -> },
    val onRelease: ReleaseActions = ReleaseActions(),
    val onGoneAgain: (GoneUi) -> Unit = {},
    val onGoneHide: (GoneUi) -> Unit = {},
    val onSaveLanguages: (audio: String?, subtitles: String?, onSaved: () -> Unit) -> Unit = { _, _, _ -> },
    val onManageReleases: () -> Unit = {},
    val onRetry: () -> Unit = {},
    val onBack: () -> Unit = {},
)

/**
 * A title of the library (web `/collection/[id]`, drawn in the TVTitleReleases language): the
 * poster and the title aside, then what to do (resume or start, keep it on the watchlist,
 * mark it watched or not, the series' languages), its episodes by season (packs, offers in each language, reclaimed
 * releases), what is on disk, what used to be, and the files. OK does an episode's main
 * action; hold OK (or a long press) lists every action of it.
 */
@Composable
fun CollectionScreen(
    container: AppContainer,
    collectionId: String,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onReplaceWithPlayer: (infohash: String, fileIdx: Int) -> Unit,
    onOpenTorrent: (infohash: String) -> Unit,
    onManageReleases: () -> Unit,
    onBack: () -> Unit,
) {
    val vm = irisViewModel(container, key = "collection:$collectionId") { c, _ -> CollectionViewModel(c, collectionId) }
    val state by vm.state.collectAsStateWithLifecycle()
    val event by vm.playEvents.collectAsStateWithLifecycle()
    RepeatWhileStarted(Unit) { vm.pollWhileStarted() }
    LaunchedEffect(event) {
        val e = event ?: return@LaunchedEffect
        vm.consumePlay()
        if (e.replace) onReplaceWithPlayer(e.infohash, e.fileIdx) else onPlay(e.infohash, e.fileIdx)
    }
    CollectionContent(
        state = state,
        lastRow = vm.lastRow,
        actions = CollectionActions(
            onPlay = onPlay,
            onEpisode = { row, action ->
                vm.lastRow = row.key
                when (action) {
                    is EpisodeAction.Play -> onPlay(action.infohash, action.fileIdx)
                    is EpisodeAction.Grab -> vm.grab(action, row.words)
                    is EpisodeAction.DownloadAgain -> vm.downloadAgain(action.gone)
                    is EpisodeAction.Hide -> vm.hide(action.infohash, row.words)
                    is EpisodeAction.MarkWatched -> vm.markWatched(action, row.words)
                }
            },
            onSeason = vm::chooseSeason,
            onWatchlist = vm::toggleWatchlist,
            onWatched = vm::toggleWatched,
            onPack = vm::grabPack,
            onRelease = ReleaseActions(
                onPlay = onPlay,
                onFiles = onOpenTorrent,
                onPause = vm::pause,
                onResume = vm::resume,
                onDelete = vm::delete,
            ),
            onGoneAgain = vm::downloadAgain,
            onGoneHide = { vm.hide(it.infohash, it.name) },
            onSaveLanguages = vm::saveLanguages,
            onManageReleases = onManageReleases,
            onRetry = vm::retry,
            onBack = onBack,
        ),
    )
}

@Composable
fun CollectionContent(
    state: CollectionUiState,
    actions: CollectionActions,
    lastRow: String? = null,
) {
    val page = state.page
    val hasEpisodes = page.valueOrNull?.episodes?.isNotEmpty() == true
    FooterLayout(
        footer = {
            ScreenFooter(
                buildList {
                    add(KeyHint(Keys.OK, if (hasEpisodes) "Play or grab" else "Choose"))
                    if (hasEpisodes) add(KeyHint(Keys.HOLD_OK, "Everything for an episode"))
                    add(KeyHint(Keys.BACK, "To the top, then the library"))
                },
            ) { NoticeLine(state.notice) }
        },
        modifier = Modifier
            .fillMaxSize()
            .background(IrisColor.ground),
    ) { footer ->
        when (page) {
            Loadable.Loading -> LoadingState(label = "Loading the title…")
            is Loadable.Failed -> ErrorState(
                page.error.message,
                actions.onRetry,
                title = if (page.error.status == 404) "This title is no longer in the library" else "Couldn't load this title",
            )
            is Loadable.Ready, is Loadable.Stale -> {
                val p = page.valueOrNull ?: return@FooterLayout
                TitlePage(p, state, actions, lastRow, page.errorOrNull, footer)
            }
        }
    }
}

private sealed interface Sheet {
    data class Episode(val row: EpisodeRowUi) : Sheet
    data object Languages : Sheet
}

@Composable
private fun TitlePage(
    p: CollectionPage,
    state: CollectionUiState,
    actions: CollectionActions,
    lastRow: String?,
    stale: studio.kahn.iris.tv.ui.state.UiError?,
    footer: Dp,
) {
    val layout = IrisLayout.current
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val playFocus = remember { FocusRequester() }
    val keys = rememberFocusReturn(fallback = playFocus)
    var atPlay by remember { mutableStateOf(false) }
    var sheet by remember { mutableStateOf<Sheet?>(null) }
    var deleting by remember { mutableStateOf<ReleaseRow?>(null) }
    val episodesStart = remember(p.packs.size) { EPISODES_FIRST + p.packs.size }
    val focusKey = lastRow?.takeIf { k -> p.episodes.any { it.key == k } }
        ?: p.episodes.getOrNull(p.opening)?.key?.takeIf { p.opening > 0 }

    LaunchedEffect(Unit) {
        val index = p.episodes.indexOfFirst { it.key == focusKey }
        val left = keys.last
        if (left != null && !left.startsWith("ep:")) {
            // Back from a release's files or the player: the row left, when it is in sight.
            snapshotFlow { list.layoutInfo.visibleItemsInfo.isNotEmpty() }.first { it }
            withFrameNanos { }
            if (keys.focusLast()) return@LaunchedEffect
        }
        if (focusKey != null && index >= 0) {
            val item = episodesStart + index
            if (list.layoutInfo.visibleItemsInfo.none { it.index == item }) list.scrollToItem(item)
            snapshotFlow { list.layoutInfo.visibleItemsInfo.any { it.index == item } }.first { it }
            keys.focus(listOf("ep:$focusKey"))
        } else {
            snapshotFlow { list.layoutInfo.visibleItemsInfo.isNotEmpty() }.first { it }
            runCatching { playFocus.requestFocus() }
        }
    }
    // A languages sheet with nothing to show is not open (it draws nothing).
    val sheetShown = when (sheet) {
        null -> false
        Sheet.Languages -> state.languages?.valueOrNull != null
        is Sheet.Episode -> true
    }
    BackHandler(enabled = !sheetShown && deleting == null) {
        if (atPlay && list.firstVisibleItemIndex == 0) {
            actions.onBack()
        } else {
            scope.launch {
                list.scrollToItem(0)
                runCatching { playFocus.requestFocus() }
            }
        }
    }

    val compact = layout.height < 500.dp
    Row(
        Modifier
            .fillMaxSize()
            .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, top = layout.safeVertical),
        horizontalArrangement = Arrangement.spacedBy(if (compact) IrisSpace.s7 else IrisSpace.s9),
    ) {
        Aside(p, compact, Modifier.width(if (compact) 120.dp else 210.dp))
        LazyColumn(
            state = list,
            modifier = Modifier
                .weight(1f)
                .fillMaxSize()
                .padding(bottom = footer),
            contentPadding = PaddingValues(top = IrisSpace.s1, bottom = IrisSpace.s4, start = IrisSpace.s2, end = IrisSpace.s2),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            item(key = "head") {
                Head(
                    p = p,
                    state = state,
                    actions = actions,
                    stale = stale,
                    onLanguages = { sheet = Sheet.Languages },
                    languagesFocus = keys.requester(LANGUAGES_KEY),
                    playFocus = playFocus,
                    onPlayFocused = { atPlay = it },
                )
            }
            if (p.showEpisodes) {
                item(key = "episodes") { EpisodesHead(p, actions.onSeason) }
                items(p.packs, key = { "pack:${it.key}" }, contentType = { "pack" }) { pack -> PackBlock(pack, state.busy, actions.onPack) }
                items(p.episodes, key = { "ep:${it.key}" }, contentType = { "episode" }) { row ->
                    EpisodeCard(
                        row = row,
                        busy = row.actions.any { it.busyKey() in state.busy },
                        onClick = {
                            val main = row.actions.firstOrNull()
                            if (main != null) actions.onEpisode(row, main) else sheet = Sheet.Episode(row)
                        },
                        onLongClick = { sheet = Sheet.Episode(row) },
                        modifier = Modifier.focusRequester(keys.requester("ep:${row.key}")),
                    )
                }
                p.emptyEpisodes?.let { words -> item(key = "no-episodes") { Hint(words) } }
            }
            item(key = "on-disk") {
                SectionTitle("On disk", meta = plural(p.onDisk.size, "release"), modifier = Modifier.padding(top = IrisSpace.s7))
            }
            items(p.onDisk, key = { "disk:${it.infohash}" }, contentType = { "disk" }) { row ->
                ReleaseItem(
                    row,
                    actions.onRelease.copy(onDelete = { deleting = it }),
                    state.busy,
                    Modifier.focusReturn(keys, "disk:${row.infohash}"),
                    showTitle = false,
                )
            }
            item(key = "manage") {
                Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
                    if (p.onDisk.isEmpty()) Hint("Nothing of this title is on disk any more.")
                    ActionButton("Manage releases", actions.onManageReleases, style = ActionStyle.Secondary, size = ActionSize.Small)
                }
            }
            state.languages?.let { langs ->
                item(key = "languages") { LanguagesBlock(langs, onChange = { sheet = Sheet.Languages }) }
            }
            if (p.showFiles) {
                item(key = "files") { SectionTitle("Files", modifier = Modifier.padding(top = IrisSpace.s7)) }
                items(p.files, key = { "file:${it.key}" }, contentType = { "file" }) { f ->
                    FileCard(f, Modifier.focusReturn(keys, "file:${f.key}")) { actions.onPlay(f.infohash, f.fileIdx) }
                }
                if (p.files.isEmpty()) item(key = "no-files") { Hint("No video file is on disk for this title.") }
            }
            if (p.gone.isNotEmpty()) {
                item(key = "gone") {
                    SectionTitle("Previously on disk", meta = plural(p.gone.size, "release"), modifier = Modifier.padding(top = IrisSpace.s7))
                }
                items(p.gone, key = { "gone:${it.infohash}" }, contentType = { "gone" }) { g -> GoneRow(g, state.busy, actions) }
                item(key = "gone-hint") { Hint("Hiding a release takes it off this page for you only. Your history is kept.") }
            }
        }
    }

    when (val s = sheet) {
        null -> Unit
        is Sheet.Episode -> EpisodeSheet(
            row = p.episodes.firstOrNull { it.key == s.row.key } ?: s.row,
            busy = state.busy,
            onAction = { row, action -> actions.onEpisode(row, action) },
            onDismiss = {
                sheet = null
                keys.returnTo("ep:${s.row.key}")
            },
        )
        Sheet.Languages -> state.languages?.valueOrNull?.let { langs ->
            LanguagesPanel(
                title = p.title,
                langs = langs,
                saving = "languages" in state.busy,
                onSave = { a, s2 ->
                    actions.onSaveLanguages(a, s2) {
                        sheet = null
                        keys.returnTo(LANGUAGES_KEY)
                    }
                },
                onDismiss = {
                    sheet = null
                    keys.returnTo(LANGUAGES_KEY)
                },
            )
        }
    }
    deleting?.let { row ->
        ConfirmDialog(
            eyebrow = "Delete a release of ${p.title}",
            title = "Delete ${row.release}?",
            body = "It leaves the disk for everyone in the house. Watch history is kept. ${row.deleteBody}",
            confirmLabel = "Delete release",
            onConfirm = {
                deleting = null
                actions.onRelease.onDelete(row)
                keys.returnTo("disk:${row.infohash}", p.onDisk.map { "disk:${it.infohash}" }, leaving = true)
            },
            onCancel = {
                deleting = null
                keys.returnTo("disk:${row.infohash}")
            },
        )
    }
}

private const val EPISODES_FIRST = 2
private const val LANGUAGES_KEY = "languages"

@Composable
private fun Aside(p: CollectionPage, compact: Boolean, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        Eyebrow("Library · ${if (p.series) "Series" else "Movie"}")
        Artwork(
            title = p.title,
            imageUrl = p.posterUrl,
            width = if (compact) 96.dp else IrisSize.posterAside,
            titleStyle = IrisType.group,
        )
        if (!compact) {
            Text(p.eyebrow, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Head(
    p: CollectionPage,
    state: CollectionUiState,
    actions: CollectionActions,
    stale: studio.kahn.iris.tv.ui.state.UiError?,
    onLanguages: () -> Unit,
    languagesFocus: FocusRequester,
    playFocus: FocusRequester,
    onPlayFocused: (Boolean) -> Unit,
) {
    // The first action takes the page's focus; the Languages action is where its panel returns.
    val playModifier = Modifier
        .focusRequester(playFocus)
        .onFocusChanged { onPlayFocused(it.hasFocus) }
    val languagesModifier = Modifier.focusRequester(languagesFocus)
    Column(verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        Text(p.title, style = IrisType.title, color = IrisColor.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Text(p.facts, style = IrisType.metaLarge, color = IrisColor.inkMuted)
        if (p.chips.isNotEmpty() || p.fresh > 0) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
                if (p.fresh > 0) Chip("${plural(p.fresh, "new episode")} since your last visit", tone = ChipTone.Ok)
                p.chips.forEach { Chip(it) }
            }
        }
        p.overview?.let {
            Text(it, style = IrisType.reading, color = IrisColor.ink, maxLines = 4, overflow = TextOverflow.Ellipsis, modifier = Modifier.widthIn(max = 560.dp))
        }
        stale?.let { StaleNotice(it) }
        FlowRow(
            Modifier.padding(top = IrisSpace.s2),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
            verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            val target = p.playTarget
            if (target != null) {
                ActionButton(
                    p.playLabel,
                    { actions.onPlay(target.infohash, target.fileIdx) },
                    icon = Icons.Rounded.PlayArrow,
                    size = ActionSize.Large,
                    modifier = playModifier,
                )
            }
            if (p.onWatchlist != null) {
                ActionButton(
                    if (p.onWatchlist) "On your watchlist" else "Add to your watchlist",
                    actions.onWatchlist,
                    icon = if (p.onWatchlist) Icons.Rounded.Check else Icons.Rounded.BookmarkBorder,
                    style = ActionStyle.Secondary,
                    size = ActionSize.Large,
                    enabled = !p.onWatchlist || p.canLeaveWatchlist,
                    busy = "watchlist" in state.busy,
                    busyText = "Saving…",
                    modifier = if (target == null) playModifier else Modifier,
                )
            }
            ActionButton(
                markWatchedLabel(p.watched),
                actions.onWatched,
                icon = if (p.watched) Icons.Rounded.RemoveDone else Icons.Rounded.CheckCircle,
                style = ActionStyle.Secondary,
                size = ActionSize.Large,
                busy = WATCHED_KEY in state.busy,
                busyText = "Saving…",
                modifier = if (target == null && p.onWatchlist == null) playModifier else Modifier,
            )
            if (state.languages != null) {
                ActionButton(
                    "Languages",
                    onLanguages,
                    icon = Icons.Rounded.Language,
                    style = ActionStyle.Secondary,
                    size = ActionSize.Large,
                    enabled = state.languages.valueOrNull != null,
                    modifier = languagesModifier,
                )
            }
        }
    }
}

@Composable
private fun EpisodesHead(p: CollectionPage, onSeason: (Long) -> Unit) {
    Column(Modifier.padding(top = IrisSpace.s7), verticalArrangement = Arrangement.spacedBy(IrisSpace.s4)) {
        SectionTitle("Episodes", meta = p.seasonFact)
        if (p.seasons.isNotEmpty()) {
            Box(Modifier.horizontalScroll(rememberScrollState())) {
                PillChoice(
                    options = p.seasons,
                    selected = p.seasons.first { it.season == p.season },
                    onSelect = { onSeason(it.season) },
                    label = { it.label },
                    modifier = Modifier.padding(IrisSpace.s1),
                )
            }
        }
    }
}

@Composable
private fun PackBlock(pack: PackUi, busy: Set<String>, onPack: (PackUi, Boolean) -> Unit) {
    FramedBlock(Modifier.fillMaxWidth()) {
        Text(pack.title, style = IrisType.bodyStrong, color = IrisColor.ink)
        Text(pack.facts, style = IrisType.meta, color = IrisColor.inkMuted)
        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3)) {
            ActionButton(
                "Grab and play",
                { onPack(pack, true) },
                icon = Icons.Rounded.PlayArrow,
                size = ActionSize.Small,
                busy = "pack-play:${pack.key}" in busy,
                busyText = "Grabbing…",
            )
            ActionButton(
                "Download only",
                { onPack(pack, false) },
                icon = Icons.Rounded.Download,
                style = ActionStyle.Secondary,
                size = ActionSize.Small,
                busy = "pack:${pack.key}" in busy,
                busyText = "Grabbing…",
            )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun EpisodeCard(
    row: EpisodeRowUi,
    busy: Boolean,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    RowCard(onClick = onClick, onLongClick = onLongClick, modifier = modifier) { focused ->
        Text(
            row.number,
            style = IrisType.section,
            color = if (focused) IrisColor.ink else IrisColor.inkMuted,
            modifier = Modifier.widthIn(min = 30.dp),
            maxLines = 1,
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
            Text(row.heading, style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            StatusLine(if (busy) "Asking the server…" else row.state.text, tone = row.state.tone)
            row.state.progress?.let { Meter(it, Modifier.widthIn(max = 240.dp)) }
            row.details.forEach { Text(it, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s1), itemVerticalAlignment = Alignment.CenterVertically) {
            row.languages.forEach { LanguageChip(it) }
        }
        row.actions.firstOrNull()?.let {
            Text(it.label, style = IrisType.controlSmall, color = if (focused) IrisColor.ink else IrisColor.accent, maxLines = 1)
        }
    }
}

@Composable
private fun FileCard(f: FileUi, modifier: Modifier = Modifier, onClick: () -> Unit) {
    RowCard(onClick = onClick, modifier = modifier) { _ ->
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
            Text(f.name, style = IrisType.mono, color = IrisColor.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(f.facts, style = IrisType.meta, color = IrisColor.inkMuted)
        }
        Text("Play", style = IrisType.controlSmall, color = IrisColor.accent)
    }
}

@Composable
private fun GoneRow(g: GoneUi, busy: Set<String>, actions: CollectionActions) {
    Row(
        Modifier
            .fillMaxWidth()
            .drawBehind {
                val y = size.height - 0.5.dp.toPx()
                drawLine(IrisColor.line, Offset(0f, y), Offset(size.width, y), strokeWidth = 1.dp.toPx())
            }
            .padding(vertical = IrisSpace.s4),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
            g.watchLine?.let { StatusLine(it, tone = if (g.watched) StatusTone.Ok else StatusTone.Info) }
            Text(g.name, style = IrisType.mono, color = IrisColor.ink, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(g.facts, style = IrisType.meta, color = IrisColor.inkMuted)
        }
        ActionButton(
            "Download again",
            { actions.onGoneAgain(g) },
            icon = Icons.Rounded.Download,
            style = ActionStyle.Secondary,
            size = ActionSize.Small,
            busy = "again:${g.infohash}" in busy,
            busyText = "Asking…",
        )
        ActionButton(
            "Hide",
            { actions.onGoneHide(g) },
            style = ActionStyle.Secondary,
            size = ActionSize.Small,
            busy = "hide:${g.infohash}" in busy,
            busyText = "Hiding…",
        )
    }
}

@Composable
private fun LanguagesBlock(langs: Loadable<LanguagesUi>, onChange: () -> Unit) {
    FramedBlock(Modifier.fillMaxWidth().padding(top = IrisSpace.s7)) {
        Text("Next episodes play with", style = IrisType.group, color = IrisColor.ink)
        when (val l = langs.valueOrNull) {
            null -> Hint(if (langs is Loadable.Failed) langs.error.message else "Loading…")
            else -> {
                FactRow("Audio", audioChoiceWords(l.audio))
                FactRow("Subtitles", subtitleChoiceWords(l.subtitles))
                Hint(if (l.forCollection) "Chosen for this series." else "Your usual choice, from your account.")
            }
        }
        ActionButton("Change languages", onChange, icon = Icons.Rounded.Language, style = ActionStyle.Secondary, size = ActionSize.Small, enabled = langs.valueOrNull != null)
    }
}

@Composable
internal fun EpisodeSheet(
    row: EpisodeRowUi,
    busy: Set<String>,
    onAction: (EpisodeRowUi, EpisodeAction) -> Unit,
    onDismiss: () -> Unit,
) {
    ActionSheet(
        title = row.title,
        actions = row.actions,
        label = { it.label },
        busyLabel = { "Asking the server…" },
        waits = { it !is EpisodeAction.Play },
        inFlight = { it.busyKey() in busy },
        onAction = { onAction(row, it) },
        onDismiss = onDismiss,
    ) {
        StatusLine(row.state.text, tone = row.state.tone)
        row.aired?.let { Text(it, style = IrisType.meta, color = IrisColor.inkMuted) }
        row.overview?.let { Text(it, style = IrisType.reading, color = IrisColor.ink, maxLines = 6, overflow = TextOverflow.Ellipsis) }
        row.details.forEach { Text(it, style = IrisType.meta, color = IrisColor.inkMuted) }
        if (row.actions.isEmpty()) Hint("Nothing to do yet: no release of this episode is known.")
    }
}

@Composable
internal fun LanguagesPanel(
    title: String,
    langs: LanguagesUi,
    saving: Boolean,
    onSave: (String?, String?) -> Unit,
    onDismiss: () -> Unit,
) {
    // The series' own choices: "" is « your usual choice », null on the wire (inherits the account's).
    var audio by remember { mutableStateOf(langs.own.audio.orEmpty()) }
    var subs by remember { mutableStateOf(langs.own.subtitles.orEmpty()) }
    val first = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { first.requestFocus() } }
    SidePanel(title = "Languages for $title", onDismiss = onDismiss) {
        Text(
            "The next episodes start with these. Other titles keep your usual choice.",
            style = IrisType.meta,
            color = IrisColor.inkMuted,
            modifier = Modifier.padding(horizontal = IrisSpace.s4),
        )
        PanelLabel("Audio")
        PanelOptions(
            options = listOf("") + langs.audioOptions,
            selected = audio,
            onSelect = { audio = it },
            label = { if (it.isEmpty()) "Your usual choice" else audioChoiceWords(it) },
            selectedFocus = first,
        )
        PanelLabel("Subtitles")
        PanelOptions(
            options = listOf("", NO_SUBTITLES) + langs.subtitleOptions,
            selected = subs,
            onSelect = { subs = it },
            label = { if (it.isEmpty()) "Your usual choice" else subtitleChoiceWords(it) },
        )
        Row(
            Modifier.padding(horizontal = IrisSpace.s4, vertical = IrisSpace.s4),
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        ) {
            ActionButton(
                "Save for this series",
                { onSave(audio.ifEmpty { null }, subs.ifEmpty { null }) },
                icon = Icons.Rounded.Check,
                busy = saving,
                busyText = "Saving…",
            )
            ActionButton("Cancel", onDismiss, style = ActionStyle.Secondary)
        }
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, style = IrisType.meta, color = IrisColor.inkMuted)
}
