package studio.kahn.iris.tv.ui.components

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import androidx.tv.material3.Border
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.Surface
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisFocus
import studio.kahn.iris.tv.ui.theme.IrisShape

/** What cannot act now is drawn at this opacity, its reason said beside it (web `--disabled-opacity`). */
const val DISABLED_ALPHA = 0.55f

/**
 * The fills of a focusable element, at rest and focused. [border] null = no frame.
 * The presets are the boards' four looks; build your own only for a new look.
 */
@Immutable
data class FocusColors(
    val container: Color,
    val content: Color,
    val border: Color?,
    val focusedContainer: Color,
    val focusedContent: Color,
    val focusedBorder: Color? = null,
) {
    companion object {
        /** A primary action: raised fill at rest, ink when focused. */
        val Filled = FocusColors(
            container = IrisColor.groundRaised,
            content = IrisColor.ink,
            border = null,
            focusedContainer = IrisColor.ink,
            focusedContent = IrisColor.ground,
        )

        /** A secondary action or a pill: a line frame at rest, ink when focused. */
        val Outlined = FocusColors(
            container = Color.Transparent,
            content = IrisColor.ink,
            border = IrisColor.line,
            focusedContainer = IrisColor.ink,
            focusedContent = IrisColor.ground,
            focusedBorder = IrisColor.ink,
        )

        /** Player controls, tabs, side-panel options: bare at rest, ink when focused. */
        val Quiet = FocusColors(
            container = Color.Transparent,
            content = IrisColor.ink,
            border = null,
            focusedContainer = IrisColor.ink,
            focusedContent = IrisColor.ground,
        )

        /** A framed row (search list, releases): raised when focused, the ring says the rest. */
        val Row = FocusColors(
            container = IrisColor.surface,
            content = IrisColor.ink,
            border = IrisColor.line,
            focusedContainer = IrisColor.groundRaised,
            focusedContent = IrisColor.ink,
        )

        /** Cards: no fill at all, the artwork carries the focus ring. */
        val None = FocusColors(
            container = Color.Transparent,
            content = IrisColor.ink,
            border = null,
            focusedContainer = Color.Transparent,
            focusedContent = IrisColor.ink,
        )
    }
}

/**
 * The accent focus ring: [IrisFocus.ringWidth] wide, [IrisFocus.ringOffset]
 * outside [bounds] (the whole element by default), following [shape].
 * Drawn outside the layout bounds, so give the parent room (lazy rows and
 * grids get it from their content padding).
 */
fun Modifier.focusRing(
    visible: Boolean,
    shape: Shape,
    bounds: ((Size) -> Rect)? = null,
): Modifier = drawWithContent {
    drawContent()
    if (!visible) return@drawWithContent
    val area = bounds?.invoke(size) ?: Rect(Offset.Zero, size)
    val width = IrisFocus.ringWidth.toPx()
    val inset = IrisFocus.ringOffset.toPx() + width / 2f
    val ringSize = Size(area.width + inset * 2f, area.height + inset * 2f)
    val outline = shape.createOutline(ringSize, layoutDirection, this)
    translate(area.left - inset, area.top - inset) {
        drawOutline(outline, IrisColor.accent, style = Stroke(width))
    }
}

/**
 * The one focusable primitive every Iris control is built on: a tv-material
 * clickable [Surface] (D-pad click and hold-OK long click) chained with
 * [touchClick] (tap and long press on a phone), plus the boards' focus
 * treatment: [colors] swap to their focused side, the accent [focusRing],
 * and an optional [focusedScale] (cards: [IrisFocus.cardScale]).
 *
 * Scale and ring are applied OUTSIDE the tv Surface (which clips to its
 * shape), so the ring can sit around a part of the element ([ringBounds],
 * e.g. only the artwork of a card) and grow with it. [modifier] lands on
 * that outer box: size it, attach a `focusRequester` or `onFocusChanged`
 * there as usual.
 *
 * [content] sits at [contentAlignment] inside the surface (which takes the
 * outer box's minimum size). It receives whether the element is focused, to swap secondary
 * colors (e.g. muted text on an ink fill becomes [IrisColor.inkMutedOnInk]).
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun FocusSurface(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    onLongClick: (() -> Unit)? = null,
    enabled: Boolean = true,
    shape: Shape = IrisShape.card,
    colors: FocusColors = FocusColors.Quiet,
    ring: Boolean = true,
    ringBounds: ((Size) -> Rect)? = null,
    focusedScale: Float = 1f,
    scaleOrigin: TransformOrigin = TransformOrigin.Center,
    interactionSource: MutableInteractionSource? = null,
    contentAlignment: Alignment = Alignment.Center,
    content: @Composable BoxScope.(focused: Boolean) -> Unit,
) {
    val source = interactionSource ?: remember { MutableInteractionSource() }
    val focused by source.collectIsFocusedAsState()
    val scale by animateFloatAsState(
        targetValue = if (focused) focusedScale else 1f,
        animationSpec = tween(IrisFocus.animationMs),
        label = "focusScale",
    )
    Box(
        modifier
            .zIndex(if (focused) 1f else 0f)
            .graphicsLayer {
                scaleX = scale
                scaleY = scale
                transformOrigin = scaleOrigin
                alpha = if (enabled) 1f else DISABLED_ALPHA
            }
            .focusRing(visible = ring && focused, shape = shape, bounds = ringBounds),
        propagateMinConstraints = true,
    ) {
        Surface(
            onClick = onClick,
            onLongClick = onLongClick,
            enabled = enabled,
            modifier = Modifier.touchClick(enabled = enabled, onLongClick = onLongClick, onClick = onClick),
            shape = ClickableSurfaceDefaults.shape(shape),
            colors = ClickableSurfaceDefaults.colors(
                containerColor = colors.container,
                contentColor = colors.content,
                focusedContainerColor = colors.focusedContainer,
                focusedContentColor = colors.focusedContent,
                pressedContainerColor = colors.focusedContainer,
                pressedContentColor = colors.focusedContent,
                disabledContainerColor = colors.container,
                disabledContentColor = colors.content,
            ),
            scale = ClickableSurfaceDefaults.scale(focusedScale = 1f, pressedScale = 1f),
            border = ClickableSurfaceDefaults.border(
                border = colors.border?.let { Border(BorderStroke(1.dp, it), shape = shape) } ?: Border.None,
                focusedBorder = colors.focusedBorder?.let { Border(BorderStroke(1.dp, it), shape = shape) }
                    ?: Border.None,
                pressedBorder = colors.focusedBorder?.let { Border(BorderStroke(1.dp, it), shape = shape) }
                    ?: Border.None,
            ),
            glow = ClickableSurfaceDefaults.glow(),
            interactionSource = source,
        ) {
            Box(Modifier.align(contentAlignment)) { content(focused) }
        }
    }
}
