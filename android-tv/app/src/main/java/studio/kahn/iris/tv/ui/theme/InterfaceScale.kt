package studio.kahn.iris.tv.ui.theme

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import studio.kahn.iris.tv.data.InterfaceSize

/** [base] grown by [size]: a dp and an sp are [InterfaceSize.scale] times as many pixels, the font scale kept. */
fun scaledDensity(base: Density, size: InterfaceSize): Density =
    if (size.scale == 1f) base else Density(base.density * size.scale, base.fontScale)

/**
 * Draws [content] at the chosen interface [size]: everything measured in dp and sp grows
 * together, and the screen holds that many fewer dp, so [IrisTheme] (inside this) lays out
 * for the smaller screen it now has. The video picture fills what it is given whatever the
 * density, and its captions are a View, sized by the system's caption settings.
 */
@Composable
fun InterfaceScale(size: InterfaceSize, content: @Composable () -> Unit) {
    val base = LocalDensity.current
    val density = remember(base, size) { scaledDensity(base, size) }
    CompositionLocalProvider(LocalDensity provides density, content = content)
}
