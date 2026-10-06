package studio.kahn.iris.tv.ui.screens.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.List
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Downloading
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.Stable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.ActionSize
import studio.kahn.iris.tv.ui.components.ActionStyle
import studio.kahn.iris.tv.ui.components.Artwork
import studio.kahn.iris.tv.ui.components.Meter
import studio.kahn.iris.tv.ui.components.PanelOption
import studio.kahn.iris.tv.ui.components.StatusLine
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.components.touchClick
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/**
 * The [StatusTone] of a poster's status line. A card is narrow: the plain states go without an
 * icon so their words fit, the words alone carry them.
 */
fun StatusTone.cardTone(): StatusTone = when (this) {
    StatusTone.Available -> StatusTone.Ok
    StatusTone.Warn -> StatusTone.Warn
    StatusTone.Down -> StatusTone.Down
    StatusTone.Ok, StatusTone.Busy, StatusTone.Info, StatusTone.Muted -> StatusTone.Muted
}

/** What a release row can do; a null callback hides the action. */
@Immutable
data class ReleaseActions(
    val onPlay: ((infohash: String, fileIdx: Int) -> Unit)? = null,
    val onFiles: ((infohash: String) -> Unit)? = null,
    val onOpenTitle: ((collectionId: String) -> Unit)? = null,
    val onPause: (ReleaseRow) -> Unit = {},
    val onResume: (ReleaseRow) -> Unit = {},
    val onDelete: (ReleaseRow) -> Unit = {},
)

/**
 * One release: its title (optional), its release name, how far it is, its state in words, its
 * facts, then what can be done: play (or resume) its one video, open its files, open its title,
 * pause or resume it, delete it. Pause, resume and delete are for an admin or whoever added it:
 * anyone else sees them not operable, the reason beside them. [actionsBeside] puts the actions
 * on the row's right (a wide list) instead of under its words.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ReleaseItem(
    row: ReleaseRow,
    actions: ReleaseActions,
    busy: Set<String>,
    modifier: Modifier = Modifier,
    showTitle: Boolean = true,
    actionsBeside: Boolean = false,
) {
    val wide = actionsBeside
    val open = actions.onOpenTitle
    val collection = row.collectionId
    // A tap on its poster or its title opens the title (as on the web); its buttons stay its own.
    val openTitle = Modifier.touchClick(enabled = open != null && collection != null) { if (open != null && collection != null) open(collection) }
    Row(
        modifier
            .fillMaxWidth()
            .drawBehind {
                val y = size.height - 0.5.dp.toPx()
                drawLine(IrisColor.line, Offset(0f, y), Offset(size.width, y), strokeWidth = 1.dp.toPx())
            }
            .padding(vertical = IrisSpace.s5),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s5),
    ) {
        if (showTitle) {
            Artwork(title = row.title, imageUrl = row.posterUrl, width = IrisSize.posterMini, showTitle = false, modifier = openTitle)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(IrisSpace.s2)) {
            if (showTitle) {
                Text(row.title, style = IrisType.bodyStrong, color = IrisColor.ink, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = openTitle)
            }
            Text(row.release, style = IrisType.mono, color = IrisColor.inkMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (row.progress != null) {
                Row(horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3), verticalAlignment = Alignment.CenterVertically) {
                    Meter(row.progress, Modifier.widthIn(max = 260.dp).weight(1f, fill = false))
                    row.progressWords?.let { Text(it, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1) }
                }
            }
            StatusLine(row.status.text, tone = row.status.tone)
            Text(row.facts, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (!wide) ReleaseButtons(row, actions, busy, Modifier.padding(top = IrisSpace.s1), end = false)
        }
        if (wide) ReleaseButtons(row, actions, busy, Modifier.widthIn(max = 380.dp), end = true)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ReleaseButtons(row: ReleaseRow, actions: ReleaseActions, busy: Set<String>, modifier: Modifier, end: Boolean) {
    FlowRow(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s3, if (end) Alignment.End else Alignment.Start),
        verticalArrangement = Arrangement.spacedBy(IrisSpace.s3),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        val play = actions.onPlay
        if (play != null && row.playIdx != null) {
            val idx = row.playIdx
            ActionButton(
                if (row.resume) "Resume" else "Play",
                { play(row.infohash, idx) },
                icon = Icons.Rounded.PlayArrow,
                size = ActionSize.Small,
            )
        }
        val files = actions.onFiles
        if (files != null && row.videoCount > 1) {
            ActionButton(
                "${row.videoCount} video files",
                { files(row.infohash) },
                icon = Icons.AutoMirrored.Rounded.List,
                style = ActionStyle.Secondary,
                size = ActionSize.Small,
            )
        }
        val open = actions.onOpenTitle
        val collection = row.collectionId
        if (open != null && collection != null) {
            ActionButton("Open the title", { open(collection) }, style = ActionStyle.Secondary, size = ActionSize.Small)
        }
        if (row.canPause || row.canResume) {
            val pausing = row.canPause
            ActionButton(
                if (pausing) "Pause" else "Resume download",
                { if (pausing) actions.onPause(row) else actions.onResume(row) },
                icon = if (pausing) Icons.Rounded.Pause else Icons.Rounded.Download,
                style = ActionStyle.Secondary,
                size = ActionSize.Small,
                enabled = row.canDelete,
                busy = (if (pausing) "pause:" else "resume:") + row.infohash in busy,
                busyText = if (pausing) "Pausing…" else "Resuming…",
            )
        }
        ActionButton(
            "Delete",
            { actions.onDelete(row) },
            icon = Icons.Rounded.Delete,
            style = ActionStyle.Secondary,
            size = ActionSize.Small,
            enabled = row.canDelete,
            busy = "delete:${row.infohash}" in busy,
            busyText = "Deleting…",
        )
        if (!row.canDelete) Text(row.noDeleteReason, style = IrisType.meta, color = IrisColor.inkMuted)
    }
}
