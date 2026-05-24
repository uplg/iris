package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.sp
import androidx.tv.material3.Text
import studio.kahn.iris.tv.ui.theme.FontDisplay
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.irisBrandGradient

/**
 * The Iris wordmark: "Iris" set in the Cal Sans display face filled with the
 * indigo→violet→fuchsia brand gradient, trailed by a dim "/" — the same lockup
 * the web header and the design mock use. Reusable so every surface that shows
 * the brand (home header, detail/search top bars) renders it identically.
 */
@Composable
fun IrisWordmark(
    modifier: Modifier = Modifier,
    fontSize: TextUnit = 34.sp,
) {
    Row(modifier, verticalAlignment = Alignment.Bottom) {
        Text(
            text = "Iris",
            style = TextStyle(
                fontFamily = FontDisplay,
                fontSize = fontSize,
                fontWeight = FontWeight.Normal,
                letterSpacing = (fontSize.value * -0.04f).sp,
                brush = irisBrandGradient(),
            ),
        )
        Text(
            text = "/",
            style = TextStyle(
                fontFamily = FontDisplay,
                fontSize = fontSize * 0.62f,
                fontWeight = FontWeight.Normal,
                color = IrisColors.FgDim,
            ),
        )
    }
}
