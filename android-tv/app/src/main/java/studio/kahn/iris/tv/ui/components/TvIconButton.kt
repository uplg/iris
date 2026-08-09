package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Border
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.Icon
import androidx.tv.material3.Surface
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors

/**
 * Circular icon-only button (web `.icon-btn`): fully transparent at rest, it
 * fills with a faint white overlay and gains the design's brand focus ring +
 * glow + lift on D-pad focus — discreet so it never competes with content.
 * Use it for persistent top-bar actions (search, settings, seedbox) where a
 * label would be noise.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun TvIconButton(
    icon: ImageVector,
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    size: Dp = 48.dp,
    iconSize: Dp = 22.dp,
    /** Small brand-colored dot on the icon's shoulder — "something new here"
     *  (e.g. an app update waiting behind Settings). Indicator only; the
     *  button behaves identically. */
    badge: Boolean = false,
) {
    Surface(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier
            .size(size)
            .touchClick(enabled = enabled, onClick = onClick),
        shape = ClickableSurfaceDefaults.shape(shape = CircleShape),
        scale = ClickableSurfaceDefaults.scale(focusedScale = Focus.controlScale),
        colors = ClickableSurfaceDefaults.colors(
            containerColor = androidx.compose.ui.graphics.Color.Transparent,
            contentColor = IrisColors.MutedForeground,
            focusedContainerColor = IrisColors.Overlay12,
            focusedContentColor = IrisColors.Foreground,
            pressedContainerColor = IrisColors.Overlay12,
            pressedContentColor = IrisColors.Foreground,
        ),
        border = ClickableSurfaceDefaults.border(
            border = Border.None,
            focusedBorder = Border(
                border = BorderStroke(Focus.ring, IrisColors.Brand),
                shape = CircleShape,
            ),
        ),
    ) {
        Box(Modifier.size(size), contentAlignment = Alignment.Center) {
            Icon(
                imageVector = icon,
                contentDescription = contentDescription,
                modifier = Modifier.size(iconSize),
            )
            if (badge) {
                Box(
                    Modifier
                        .align(Alignment.TopEnd)
                        .padding(top = 9.dp, end = 9.dp)
                        .size(8.dp)
                        .background(IrisColors.Brand, CircleShape),
                )
            }
        }
    }
}

