package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Border
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.Icon
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.irisPrimaryFill

enum class IrisButtonVariant { Primary, Ghost }

/**
 * The Iris action button (web `.btn`). Two variants:
 *
 *   * **Primary** — the indigo→violet vertical-sheen fill with dark ink, used
 *     for the single most important action on a screen (Play / Resume).
 *   * **Ghost** — a faint white-overlay fill with a hairline border for
 *     secondary actions (More info, Watchlist, Back).
 *
 * Focus is the shared design-system treatment: a brand ring, a brand glow
 * behind the surface, and a small lift (`scale`). Built on the TV-Material
 * clickable `Surface` so D-pad focus, scale and glow are all native.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun IrisButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    variant: IrisButtonVariant = IrisButtonVariant.Primary,
    icon: ImageVector? = null,
    enabled: Boolean = true,
    /** Focus pop. Pass `1f` inside dense action rows (episode variant chips,
     *  season packs) where a growing neighbour would shove the row around —
     *  the brand ring + glow still signal focus. */
    focusedScale: Float = Focus.controlScale,
) {
    val primary = variant == IrisButtonVariant.Primary
    val shape = RoundedCornerShape(Radius.button)

    Surface(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier,
        shape = ClickableSurfaceDefaults.shape(shape),
        scale = ClickableSurfaceDefaults.scale(focusedScale = focusedScale),
        colors = ClickableSurfaceDefaults.colors(
            // Primary draws its gradient inside the content Row, so the surface
            // container stays transparent; ghost uses a flat overlay fill.
            containerColor = if (primary) Color.Transparent else IrisColors.Overlay06,
            contentColor = if (primary) IrisColors.OnBrand else IrisColors.Foreground,
            focusedContainerColor = if (primary) Color.Transparent else IrisColors.Overlay12,
            focusedContentColor = if (primary) IrisColors.OnBrand else IrisColors.Foreground,
            pressedContainerColor = if (primary) Color.Transparent else IrisColors.Overlay12,
            pressedContentColor = if (primary) IrisColors.OnBrand else IrisColors.Foreground,
        ),
        border = ClickableSurfaceDefaults.border(
            border = Border(
                border = BorderStroke(1.dp, if (primary) Color.Transparent else IrisColors.Border),
                shape = shape,
            ),
            focusedBorder = Border(
                // A brand ring on the brand-filled primary button is
                // violet-on-violet (invisible) — use a light ring there;
                // brand reads fine against the ghost's dark fill.
                border = BorderStroke(Focus.ring, if (primary) IrisColors.Foreground else IrisColors.Brand),
                shape = shape,
            ),
        ),
    ) {
        Row(
            modifier = if (primary) {
                Modifier.background(irisPrimaryFill(), shape)
            } else {
                Modifier
            }.padding(horizontal = 24.dp, vertical = 14.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (icon != null) {
                Icon(
                    imageVector = icon,
                    contentDescription = null,
                    modifier = Modifier.size(22.dp),
                )
            }
            Text(text, style = MaterialTheme.typography.titleSmall)
        }
    }
}
