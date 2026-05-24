package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.FontMono
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Radius

/**
 * Uppercase, wide-tracked eyebrow label — the small accent line that sits
 * above hero/section titles (web `.eyebrow`). Brand-tinted by default to match
 * the hero "CONTINUE TONIGHT", dim for neutral section eyebrows.
 */
@Composable
fun Eyebrow(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = IrisColors.FgDim,
) {
    Text(
        text = text.uppercase(),
        modifier = modifier,
        style = MaterialTheme.typography.labelMedium,
        color = color,
    )
}

/**
 * Section / shelf heading in the Cal Sans display face (web `.shelf-head h2`,
 * `.section-head h2`). Pair with [Eyebrow] above it for the full lockup.
 */
@Composable
fun SectionTitle(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = IrisColors.Foreground,
) {
    Text(
        text = text,
        modifier = modifier,
        style = MaterialTheme.typography.headlineMedium,
        color = color,
    )
}

/** A 4 dp dim separator dot for meta rows (web `.hero-meta .dot`). */
@Composable
fun MetaDot(modifier: Modifier = Modifier) {
    Box(
        modifier
            .size(4.dp)
            .background(IrisColors.FgDim, RoundedCornerShape(Radius.pill)),
    )
}

/**
 * Boxed mono metadata pill — the "4K · HDR" / "VFF · VOSTFR" capsules with a
 * hairline border (web `.hero-meta .pill`). Mono face so technical tokens line
 * up; foreground ink on a transparent fill.
 */
@Composable
fun MetaPill(
    text: String,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier
            .border(BorderStroke(1.dp, IrisColors.BorderStrong), RoundedCornerShape(Radius.md))
            .padding(horizontal = 10.dp, vertical = 3.dp),
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelSmall.copy(fontFamily = FontMono),
            color = IrisColors.Foreground,
        )
    }
}

/**
 * Rounded mono chip used for ratings / genres on the detail screen
 * (web `.chip`). [brand] swaps the neutral white-overlay fill for the soft
 * brand tint + brand ink.
 */
@Composable
fun Chip(
    text: String,
    modifier: Modifier = Modifier,
    brand: Boolean = false,
) {
    val fill = if (brand) IrisColors.BrandSoft else IrisColors.Overlay06
    val stroke = if (brand) IrisColors.Brand.copy(alpha = 0.3f) else IrisColors.Border
    val ink = if (brand) IrisColors.Brand else IrisColors.Foreground
    Box(
        modifier
            .background(fill, RoundedCornerShape(Radius.pill))
            .border(BorderStroke(1.dp, stroke), RoundedCornerShape(Radius.pill))
            .padding(horizontal = 14.dp, vertical = 5.dp),
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelSmall.copy(
                fontFamily = FontMono,
                fontWeight = FontWeight.Medium,
            ),
            color = ink,
        )
    }
}
