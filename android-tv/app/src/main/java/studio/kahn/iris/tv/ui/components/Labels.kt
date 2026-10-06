package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Block
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.Downloading
import androidx.compose.material.icons.rounded.Error
import androidx.compose.material.icons.rounded.Info
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** The uppercase kicker above a title ("Continue where you left off"). */
@Composable
fun Eyebrow(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = IrisColor.inkMuted,
) {
    Text(text.uppercase(), modifier = modifier, style = IrisType.eyebrow, color = color, maxLines = 1)
}

/**
 * A row or section heading ([IrisType.section], 32 px), with an
 * optional muted [meta] beside it on the same baseline ("24 found · 3 trackers
 * answered"). Announced as a heading.
 */
@Composable
fun SectionTitle(
    text: String,
    modifier: Modifier = Modifier,
    meta: String? = null,
    style: TextStyle = IrisType.section,
    color: Color = IrisColor.ink,
) {
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
    ) {
        Text(
            text,
            style = style,
            color = color,
            modifier = Modifier
                .alignByBaseline()
                .semantics { heading() },
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        if (meta != null) {
            Text(meta, style = IrisType.meta, color = IrisColor.inkMuted, modifier = Modifier.alignByBaseline(), maxLines = 1)
        }
    }
}

/**
 * A share bar, 0..1 (watched part, downloaded part). Decorative: hidden from
 * accessibility, so always put the value in words beside it ("23 min left",
 * "64%").
 */
@Composable
fun Meter(
    fraction: Float,
    modifier: Modifier = Modifier,
    color: Color = IrisColor.accent,
    track: Color = IrisColor.groundRaised,
    height: Dp = IrisSize.meter,
) {
    Box(
        modifier
            .fillMaxWidth()
            .height(height)
            .clip(IrisShape.pill)
            .background(track)
            .clearAndSetSemantics {},
    ) {
        Box(
            Modifier
                .fillMaxHeight()
                .fillMaxWidth(fraction.coerceIn(0f, 1f))
                .background(color),
        )
    }
}

/** The tone of a [StatusLine]. Each but [Muted] brings an icon, so the words never stand on color alone. */
enum class StatusTone(val color: Color, val icon: ImageVector?) {
    /** Done, on disk, saved. */
    Ok(IrisColor.accent, Icons.Rounded.CheckCircle),
    Muted(IrisColor.inkMuted, null),
    Warn(IrisColor.warn, Icons.Rounded.Error),
    Down(IrisColor.down, Icons.Rounded.Block),
    /** Moving: downloading, getting ready. */
    Busy(IrisColor.accent, Icons.Rounded.Downloading),
    /** Not here yet, one press away (a release to grab). */
    Available(IrisColor.accent, Icons.Rounded.Download),
    /** A fact, not a state (in progress, watched earlier). */
    Info(IrisColor.inkMuted, Icons.Rounded.Info),
}

/** A card's or row's state in words: "In your library", "9 of 18 on disk", "No release found". */
@Composable
fun StatusLine(
    text: String,
    modifier: Modifier = Modifier,
    tone: StatusTone = StatusTone.Muted,
    style: TextStyle = IrisType.meta,
    icon: ImageVector? = tone.icon,
    maxLines: Int = 1,
) {
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            Icon(icon, contentDescription = null, tint = tone.color, modifier = Modifier.size(IrisSize.iconSmall))
        }
        Text(text, style = style, color = tone.color, maxLines = maxLines, overflow = TextOverflow.Ellipsis)
    }
}

/** One fact of a release sheet (TVRelease): a muted label column, then the value; a line below. */
@Composable
fun FactRow(
    label: String,
    value: String,
    modifier: Modifier = Modifier,
) {
    FactRow(label, modifier) {
        Text(value, style = IrisType.body, color = IrisColor.ink)
    }
}

/** [FactRow] with a free value slot (chips, a status line). */
@Composable
fun FactRow(
    label: String,
    modifier: Modifier = Modifier,
    value: @Composable RowScope.() -> Unit,
) {
    Row(
        modifier
            .fillMaxWidth()
            .drawBehind {
                val y = size.height - 0.5.dp.toPx()
                drawLine(IrisColor.line, Offset(0f, y), Offset(size.width, y), strokeWidth = 1.dp.toPx())
            }
            .padding(vertical = IrisSpace.s3),
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s4),
    ) {
        Text(label, style = IrisType.meta, color = IrisColor.inkMuted, modifier = Modifier.widthIn(min = IrisSize.factLabel, max = IrisSize.factLabel))
        value()
    }
}

/** The key names used in [KeyHints], as the boards write them. */
object Keys {
    const val OK = "OK"
    const val HOLD_OK = "Hold OK"
    const val BACK = "Back"
    const val LEFT_RIGHT = "← →"
    const val LEFT = "←"
    const val RIGHT = "→"
    const val UP = "↑"
    const val DOWN = "↓"
}

/** One remote hint: a key cap and what it does here. */
@Immutable
data class KeyHint(val key: String, val words: String)

/** A remote key drawn as a cap ("OK", "Back"). */
@Composable
fun KeyCap(key: String, modifier: Modifier = Modifier, onStage: Boolean = false) {
    Box(
        modifier
            .heightIn(min = IrisSize.keyCapHeight)
            .widthIn(min = IrisSize.keyCapMinWidth)
            .border(1.dp, if (onStage) IrisColor.stageFill else IrisColor.line, IrisShape.key)
            .padding(horizontal = 5.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(key, style = IrisType.key, color = IrisColor.ink, maxLines = 1)
    }
}

/**
 * The footer every board ends with: what the remote keys do on this screen,
 * then an optional [trailing] note at the far end ("Row 1 of 10").
 * Place it 15 dp above the bottom edge, inside the safe margins; [framed]
 * gives it the library's full-width band (ground fill + a line on top, 46 dp).
 * [onStage] = over the player.
 */
@Composable
fun KeyHints(
    hints: List<KeyHint>,
    modifier: Modifier = Modifier,
    trailing: String? = null,
    framed: Boolean = false,
    onStage: Boolean = false,
) {
    val base = if (framed) {
        modifier
            .fillMaxWidth()
            .heightIn(min = 46.dp)
            .background(IrisColor.ground)
            .drawBehind {
                drawLine(IrisColor.line, Offset(0f, 0.5.dp.toPx()), Offset(size.width, 0.5.dp.toPx()), strokeWidth = 1.dp.toPx())
            }
            .padding(horizontal = IrisLayout.current.safeHorizontal)
    } else {
        modifier.fillMaxWidth()
    }
    Row(
        base,
        horizontalArrangement = Arrangement.spacedBy(IrisSpace.s7),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        hints.forEach { hint ->
            Row(
                horizontalArrangement = Arrangement.spacedBy(IrisSpace.s2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                KeyCap(hint.key, onStage = onStage)
                Text(hint.words, style = IrisType.meta, color = if (onStage) IrisColor.stageMuted else IrisColor.inkMuted, maxLines = 1)
            }
        }
        if (trailing != null) {
            Spacer(Modifier.weight(1f))
            Text(trailing, style = IrisType.meta, color = IrisColor.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}
