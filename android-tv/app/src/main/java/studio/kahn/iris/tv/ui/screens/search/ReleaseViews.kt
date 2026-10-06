package studio.kahn.iris.tv.ui.screens.search

import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowForward
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter
import studio.kahn.iris.tv.data.LibraryMatch
import studio.kahn.iris.tv.data.ProviderResultMeta
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Artwork
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.ChipSize
import studio.kahn.iris.tv.ui.components.ChipTone
import studio.kahn.iris.tv.ui.components.ConfirmDialog
import studio.kahn.iris.tv.ui.components.FocusReturn
import studio.kahn.iris.tv.ui.components.FramedBlock
import studio.kahn.iris.tv.ui.components.focusReturn
import studio.kahn.iris.tv.ui.components.PosterCard
import studio.kahn.iris.tv.ui.components.RowCard
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.format.codecWord
import studio.kahn.iris.tv.ui.format.kindWord
import studio.kahn.iris.tv.ui.format.languageLabel

/**
 * One release as a list row (TVSearchList, TVTitleReleases): what it is in
 * words, its name as the tracker wrote it, its facts (the dead-swarm warning
 * in words), and its state as chips. [withTitle] false on a title's own page
 * (the title is the page's). OK opens it, hold OK grabs and plays.
 */
@Composable
fun ReleaseRow(
    r: SearchResult,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
    modifier: Modifier = Modifier,
    withTitle: Boolean = true,
    busy: Boolean = false,
) {
    val owned = ownedFile(r) != null
    val dead = isDead(r.seeders) && !owned
    RowCard(onClick = onClick, onLongClick = onLongClick, modifier = modifier) { _ ->
        if (withTitle) {
            Artwork(
                title = titleOf(r),
                imageUrl = r.posterUrl,
                width = IrisSize.posterMini,
                showTitle = false,
                framed = true,
            )
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            val what = listOfNotNull(
                if (withTitle) titleOf(r) else null,
                partWords(r.parsedSeason, r.parsedEpisode, r.title),
                languageLabel(r.languageTag),
            ).joinToString(" · ").ifEmpty { titleOf(r) }
            Text(what, style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(r.title, style = IrisType.mono, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val facts = listOfNotNull(resolution(r.title), codecWord(r.codec), factsLine(r).ifEmpty { null }).joinToString(" · ")
            when {
                busy -> Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s1), verticalAlignment = Alignment.CenterVertically) {
                    Spinner(Modifier.size(IrisSize.iconSmall), color = IrisColor.accent)
                    Text("Starting the download…", style = IrisType.meta, color = IrisColor.accent)
                }
                dead -> StatusLine("$DEAD · $facts", tone = StatusTone.Warn)
                else -> Text(facts, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            if (owned) Chip("In your library", tone = ChipTone.Ok, size = ChipSize.Small)
            if (r.freeleech == true) Chip("Freeleech", size = ChipSize.Small)
        }
    }
}

/** One release as a poster (TVSearchGrid): the tracker on the art, what and how below, its state in words. */
@Composable
fun ReleaseCard(
    r: SearchResult,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
    modifier: Modifier = Modifier,
    busy: Boolean = false,
) {
    val owned = ownedFile(r) != null
    val how = listOfNotNull(resolution(r.title), seedersWords(r.seeders)).joinToString(" · ").ifEmpty { null }
    val (status, tone) = when {
        busy -> "Starting the download…" to StatusTone.Muted
        owned -> "In your library" to StatusTone.Ok
        isDead(r.seeders) -> DEAD to StatusTone.Warn
        else -> how to StatusTone.Muted
    }
    PosterCard(
        title = titleOf(r),
        imageUrl = r.posterUrl,
        onClick = onClick,
        onLongClick = onLongClick,
        modifier = modifier,
        width = null,
        badge = r.providerId,
        meta = gridWhat(r),
        status = status,
        statusTone = tone,
    )
}

/** What the library already holds for this search (TVSearch "In your library"): its way in. */
@Composable
fun LibraryMatchRow(
    m: LibraryMatch,
    onGo: (MatchTarget) -> Unit,
    modifier: Modifier = Modifier,
) {
    val target = matchTarget(m)
    FramedBlock(modifier.fillMaxWidth()) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Artwork(
                title = m.displayTitle,
                imageUrl = tmdbPosterUrl(m.posterPath, "w185"),
                width = IrisSize.posterMini,
                showTitle = false,
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(m.displayTitle, style = IrisType.group, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOfNotNull(kindWord(m.kind), target.facts).joinToString(" · "),
                    style = IrisType.meta,
                    color = IrisColor.inkMuted,
                    maxLines = 2,
                )
            }
            ActionButton(
                text = target.action,
                onClick = { onGo(target) },
                icon = if (target is MatchTarget.Play) Icons.Rounded.PlayArrow else Icons.AutoMirrored.Rounded.ArrowForward,
            )
        }
    }
}

/**
 * The results' summary line ("24 releases from 3 trackers · c411 did not
 * answer"), the missing trackers said in warning words, each with a way to
 * ask again (the search asks every tracker at once, so a retry asks them all).
 */
@Composable
fun SummaryLine(words: String?, failed: List<ProviderResultMeta>, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    if (words.isNullOrEmpty() && failed.isEmpty()) return
    Row(
        modifier.semantics { liveRegion = LiveRegionMode.Polite },
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (failed.isEmpty()) {
            Text(words.orEmpty(), style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        } else {
            StatusLine(words.orEmpty().ifEmpty { "${failed.joinToString(", ") { it.id }} did not answer" }, tone = StatusTone.Warn)
            failed.forEach { p ->
                ActionButton("Retry ${p.id}", onRetry, style = ActionStyle.Secondary, size = ActionSize.Small, icon = Icons.Rounded.Refresh)
            }
        }
    }
}

/** A grab that did not start, said above the results until closed. */
@Composable
fun GrabRefusal(grab: GrabUi, onClose: () -> Unit, modifier: Modifier = Modifier) {
    val refused = grab as? GrabUi.Refused ?: return
    val close = remember { FocusRequester() }
    LaunchedEffect(refused) { runCatching { close.requestFocus() } }
    FramedBlock(modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Assertive }) {
        Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s1)) {
                StatusLine("Nothing was downloaded", tone = StatusTone.Down, style = IrisType.bodyStrong)
                Text(refused.message, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 3, overflow = TextOverflow.Ellipsis)
            }
            ActionButton("Close", onClose, style = ActionStyle.Secondary, size = ActionSize.Small, modifier = Modifier.focusRequester(close))
        }
    }
}

/** The asks of a grab (a huge pack, a second copy of a movie) as a confirmation. */
@Composable
fun GrabAsk(grab: GrabUi, onConfirm: () -> Unit, onCancel: () -> Unit) {
    when (grab) {
        is GrabUi.AskHuge -> ConfirmDialog(
            eyebrow = "Before downloading",
            title = grab.title,
            body = grab.body,
            confirmLabel = grab.confirm,
            onConfirm = onConfirm,
            onCancel = onCancel,
        )
        is GrabUi.AskDuplicate -> ConfirmDialog(
            eyebrow = "Before downloading",
            title = grab.title,
            confirmLabel = "Download another copy",
            onConfirm = onConfirm,
            onCancel = onCancel,
        )
        else -> Unit
    }
}

/** The end of a paged list: the next page loading, its failure with a retry, or the end said. */
@Composable
fun MoreFooter(loadingMore: Boolean, error: UiError?, hasNext: Boolean, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().heightIn(min = IrisSize.control), contentAlignment = Alignment.Center) {
        when {
            error != null -> Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3), verticalAlignment = Alignment.CenterVertically) {
                StatusLine("The next page did not load: ${error.message}", tone = StatusTone.Down)
                ActionButton("Try again", onRetry, style = ActionStyle.Secondary, size = ActionSize.Small, icon = Icons.Rounded.Refresh)
            }
            loadingMore || hasNext -> Row(
                Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Spinner(Modifier.size(10.dp), color = IrisColor.inkMuted)
                Text("Loading more releases…", style = IrisType.meta, color = IrisColor.inkMuted)
            }
            else -> Text("That is every release the trackers sent.", style = IrisType.meta, color = IrisColor.inkMuted)
        }
    }
}

/** Asks for the next page when the last items come into view: driven by scrolling, no timer. */
@Composable
fun LoadMoreAtEnd(state: LazyListState, enabled: Boolean, onLoadMore: () -> Unit) {
    val load by rememberUpdatedState(onLoadMore)
    LaunchedEffect(state, enabled) {
        if (!enabled) return@LaunchedEffect
        snapshotFlow {
            val info = state.layoutInfo
            (info.visibleItemsInfo.lastOrNull()?.index ?: 0) >= info.totalItemsCount - LOOKAHEAD
        }.distinctUntilChanged().filter { it }.collect { load() }
    }
}

@Composable
fun LoadMoreAtEnd(state: LazyGridState, enabled: Boolean, onLoadMore: () -> Unit) {
    val load by rememberUpdatedState(onLoadMore)
    LaunchedEffect(state, enabled) {
        if (!enabled) return@LaunchedEffect
        snapshotFlow {
            val info = state.layoutInfo
            (info.visibleItemsInfo.lastOrNull()?.index ?: 0) >= info.totalItemsCount - LOOKAHEAD * 2
        }.distinctUntilChanged().filter { it }.collect { load() }
    }
}

private const val LOOKAHEAD = 4

/**
 * A vertical list of rows with the [studio.kahn.iris.tv.ui.components.CardRow]
 * contract: stable keys, focus restored to the row focused last.
 */
@Composable
fun ReleaseList(
    state: LazyListState,
    modifier: Modifier = Modifier,
    contentPadding: PaddingValues = PaddingValues(4.dp),
    content: LazyListScope.() -> Unit,
) {
    LazyColumn(
        modifier = modifier
            .focusRestorer()
            .focusGroup(),
        state = state,
        contentPadding = contentPadding,
        verticalArrangement = Arrangement.spacedBy(7.dp),
        content = content,
    )
}

/** The rows of [releases] with their stable keys. */
fun LazyListScope.releaseRows(
    releases: List<SearchResult>,
    grab: GrabUi,
    remembered: FocusReturn,
    onOpen: (SearchResult) -> Unit,
    onGrab: (SearchResult) -> Unit,
    withTitle: Boolean = true,
) {
    items(releases, key = ::releaseKey) { r ->
        val key = releaseKey(r)
        ReleaseRow(
            r = r,
            onClick = { onOpen(r) },
            onLongClick = { onGrab(r) },
            withTitle = withTitle,
            busy = grab is GrabUi.Busy && grab.key == key,
            modifier = Modifier.focusReturn(remembered, key),
        )
    }
}
