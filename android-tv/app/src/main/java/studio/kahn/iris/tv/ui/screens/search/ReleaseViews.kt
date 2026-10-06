package studio.kahn.iris.tv.ui.screens.search

import studio.kahn.iris.tv.ui.format.seedersWords
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowForward
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
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
import studio.kahn.iris.tv.ui.components.PageEnd
import studio.kahn.iris.tv.ui.components.focusReturn
import studio.kahn.iris.tv.ui.components.PosterCard
import studio.kahn.iris.tv.ui.components.RowCard
import studio.kahn.iris.tv.ui.components.Spinner
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.touchClick
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType
import studio.kahn.iris.tv.ui.format.codecWord
import studio.kahn.iris.tv.ui.format.kindWord
import studio.kahn.iris.tv.ui.format.languageLabel
import studio.kahn.iris.tv.ui.components.Notice
import studio.kahn.iris.tv.ui.components.NoticeLine

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
    // A tap anywhere but its button opens the title (the web card opens from its poster too).
    FramedBlock(modifier.fillMaxWidth().touchClick { onGo(MatchTarget.Open(m.collectionId, target.facts)) }) {
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
    NoticeLine(Notice("Nothing was downloaded", StatusTone.Down), modifier, detail = refused.message, onClose = onClose, takeFocus = false)
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

/**
 * The end of the releases: the next page loading, its failure with a retry, "Show more
 * releases" when the last page showed nothing new for the filters, or the end said.
 */
@Composable
fun MoreFooter(pages: ReleasePages, onRetry: () -> Unit, onMore: () -> Unit, modifier: Modifier = Modifier) {
    PageEnd(
        loadingMore = pages.loadingMore,
        error = pages.moreError,
        hasNext = pages.hasNext,
        onRetry = onRetry,
        loadingText = "Loading more releases…",
        endText = "That is every release the trackers sent.",
        modifier = modifier,
        onMore = onMore.takeIf { pages.waitsForViewer },
        moreText = "Show more releases",
    )
}

/**
 * Focus back where a grab's ask or refusal was opened from, once it closes (a confirm, a
 * huge pack, a duplicate, "Nothing was downloaded"): the release row or card of [grab]'s key.
 */
@Composable
fun ReturnFocusAfterGrab(grab: GrabUi, focus: FocusReturn) {
    var asked by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(grab) {
        when (grab) {
            is GrabUi.AskHuge, is GrabUi.AskDuplicate, is GrabUi.Refused -> asked = grab.key
            GrabUi.Idle -> {
                asked?.let { focus.returnTo(it) }
                asked = null
            }
            is GrabUi.Busy -> Unit
        }
    }
}

/** A library match in a lazy list: its key, tagged so focus comes back to it from the player. */
fun matchKey(m: LibraryMatch): String = "match-${m.collectionId}"

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
