package studio.kahn.iris.tv.ui.components

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.inset
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Icon
import androidx.tv.material3.LocalContentColor
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisSize
import studio.kahn.iris.tv.ui.theme.IrisSpace
import studio.kahn.iris.tv.ui.theme.IrisType

/** The three looks of an [ActionButton]. Focused, all three fill with ink. */
enum class ActionStyle(val colors: FocusColors) {
    /** The one main action of a screen (Resume, Download and play): raised fill at rest. */
    Primary(FocusColors.Filled),
    /** Every other action: a line frame at rest. */
    Secondary(FocusColors.Outlined),
    /** On the player stage: bare at rest. */
    Quiet(FocusColors.Quiet),
}

/** Board heights: 72 px (a page's actions), 64 px (home, getting ready), 56 px (inside a panel). */
enum class ActionSize(val height: Dp, val padding: Dp, val iconSize: Dp, val text: TextStyle) {
    Large(IrisSize.controlLarge, 16.dp, IrisSize.icon, IrisType.action),
    Regular(IrisSize.control, 14.dp, 13.dp, IrisType.action),
    Small(IrisSize.controlSmall, 12.dp, 12.dp, IrisType.controlSmall),
}

/**
 * A labelled action (a pill). Always words, an optional leading [icon].
 *
 * [busy] keeps the button in place and focusable but swaps the icon for a
 * [Spinner] and the label for [busyText] (say what is happening: "Starting…",
 * "Deleting…"); presses are ignored meanwhile. [enabled] false dims it: say
 * why beside it. [onLongClick] fires on hold OK and on a touch long press.
 */
@Composable
fun ActionButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    style: ActionStyle = ActionStyle.Primary,
    icon: ImageVector? = null,
    enabled: Boolean = true,
    busy: Boolean = false,
    busyText: String? = null,
    size: ActionSize = ActionSize.Regular,
    onLongClick: (() -> Unit)? = null,
) {
    FocusSurface(
        onClick = { if (!busy) onClick() },
        onLongClick = onLongClick?.let { long -> { if (!busy) long() } },
        enabled = enabled,
        modifier = modifier.heightIn(min = size.height),
        shape = IrisShape.pill,
        colors = style.colors,
    ) {
        Row(
            Modifier.padding(horizontal = size.padding),
            horizontalArrangement = Arrangement.spacedBy(7.dp, Alignment.CenterHorizontally),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            when {
                busy -> Spinner(Modifier.size(size.iconSize))
                icon != null -> Icon(icon, contentDescription = null, modifier = Modifier.size(size.iconSize))
            }
            Text(
                text = if (busy) busyText ?: text else text,
                style = size.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/**
 * An icon-only round action (player controls, a header shortcut).
 * [contentDescription] is required: it is the only words the control has.
 * [badge] adds a small accent dot ("something new here"); say what in the
 * description too, e.g. "Settings, update available".
 */
@Composable
fun IconAction(
    icon: ImageVector,
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    size: Dp = IrisSize.controlLarge,
    style: ActionStyle = ActionStyle.Quiet,
    badge: Boolean = false,
    onLongClick: (() -> Unit)? = null,
) {
    FocusSurface(
        onClick = onClick,
        onLongClick = onLongClick,
        enabled = enabled,
        modifier = modifier
            .size(size)
            .semantics { this.contentDescription = contentDescription },
        shape = IrisShape.circle,
        colors = style.colors,
    ) {
        Box(Modifier.size(size), contentAlignment = Alignment.Center) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(size * 0.4f))
            if (badge) {
                Box(
                    Modifier
                        .align(Alignment.TopEnd)
                        .padding(top = size * 0.18f, end = size * 0.18f)
                        .size(IrisSpace.s1)
                        .background(IrisColor.accent, IrisShape.circle),
                )
            }
        }
    }
}

/**
 * An indeterminate activity mark, in the current content color. Decorative:
 * always say in words what is going on next to it.
 */
@Composable
fun Spinner(
    modifier: Modifier = Modifier,
    color: Color = LocalContentColor.current,
    strokeWidth: Dp = 2.dp,
) {
    val transition = rememberInfiniteTransition(label = "spinner")
    val angle by transition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(tween(900, easing = LinearEasing), RepeatMode.Restart),
        label = "spinnerAngle",
    )
    Canvas(modifier.clearAndSetSemantics {}) {
        val stroke = strokeWidth.toPx()
        inset(stroke / 2f) {
            drawArc(
                color = color.copy(alpha = 0.3f),
                startAngle = 0f,
                sweepAngle = 360f,
                useCenter = false,
                style = Stroke(stroke),
            )
            rotate(angle) {
                drawArc(
                    color = color,
                    startAngle = -90f,
                    sweepAngle = 110f,
                    useCenter = false,
                    style = Stroke(stroke, cap = StrokeCap.Round),
                )
            }
        }
    }
}
