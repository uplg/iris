package studio.kahn.iris.tv.ui.theme

import androidx.compose.material3.LocalTextStyle as M3LocalTextStyle
import androidx.compose.material3.MaterialTheme as M3MaterialTheme
import androidx.compose.material3.darkColorScheme as m3DarkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.tv.material3.LocalContentColor
import androidx.tv.material3.LocalTextStyle
import androidx.tv.material3.MaterialTheme as TvMaterialTheme
import androidx.tv.material3.darkColorScheme as tvDarkColorScheme

private val IrisTvColors = tvDarkColorScheme(
    primary = IrisColor.accent,
    onPrimary = IrisColor.onAccent,
    primaryContainer = IrisColor.accentWash,
    onPrimaryContainer = IrisColor.accent,
    secondary = IrisColor.inkMuted,
    onSecondary = IrisColor.ground,
    secondaryContainer = IrisColor.groundRaised,
    onSecondaryContainer = IrisColor.ink,
    tertiary = IrisColor.warn,
    onTertiary = IrisColor.ground,
    background = IrisColor.ground,
    onBackground = IrisColor.ink,
    surface = IrisColor.surface,
    onSurface = IrisColor.ink,
    surfaceVariant = IrisColor.groundRaised,
    onSurfaceVariant = IrisColor.inkMuted,
    inverseSurface = IrisColor.ink,
    inverseOnSurface = IrisColor.ground,
    inversePrimary = IrisColor.accentSoft,
    surfaceTint = IrisColor.surface,
    error = IrisColor.down,
    onError = IrisColor.ground,
    errorContainer = IrisColor.downWash,
    onErrorContainer = IrisColor.down,
    border = IrisColor.line,
    borderVariant = IrisColor.line,
    scrim = IrisColor.overlay,
)

// Mobile Material3 stays for the phone build of the same APK (text fields
// with the soft keyboard); it reads the very same tokens.
private val IrisM3Colors = m3DarkColorScheme(
    primary = IrisColor.accent,
    onPrimary = IrisColor.onAccent,
    primaryContainer = IrisColor.accentWash,
    onPrimaryContainer = IrisColor.accent,
    secondary = IrisColor.inkMuted,
    onSecondary = IrisColor.ground,
    secondaryContainer = IrisColor.groundRaised,
    onSecondaryContainer = IrisColor.ink,
    tertiary = IrisColor.warn,
    onTertiary = IrisColor.ground,
    background = IrisColor.ground,
    onBackground = IrisColor.ink,
    surface = IrisColor.surface,
    onSurface = IrisColor.ink,
    surfaceVariant = IrisColor.groundRaised,
    onSurfaceVariant = IrisColor.inkMuted,
    surfaceTint = IrisColor.surface,
    surfaceContainerLowest = IrisColor.ground,
    surfaceContainerLow = IrisColor.surface,
    surfaceContainer = IrisColor.surface,
    surfaceContainerHigh = IrisColor.groundRaised,
    surfaceContainerHighest = IrisColor.groundRaised,
    inverseSurface = IrisColor.ink,
    inverseOnSurface = IrisColor.ground,
    inversePrimary = IrisColor.accentSoft,
    error = IrisColor.down,
    onError = IrisColor.ground,
    errorContainer = IrisColor.downWash,
    onErrorContainer = IrisColor.down,
    outline = IrisColor.line,
    outlineVariant = IrisColor.line,
    scrim = IrisColor.overlay,
)

/**
 * The Iris theme: tv-material and mobile Material3 both read [IrisColor],
 * default text is [IrisType.body] in [IrisColor.ink], and [IrisLayout] is
 * measured from the window. Wrap every screen (and every screenshot) in it.
 * [layoutOverride] is for screenshot tests that render a phone-sized frame.
 */
@Composable
fun IrisTheme(layoutOverride: IrisLayout? = null, content: @Composable () -> Unit) {
    val window = LocalWindowInfo.current.containerSize
    val density = LocalDensity.current
    val layout = layoutOverride ?: remember(window, density) {
        if (window.width == 0 || window.height == 0) {
            IrisLayout.Tv
        } else {
            with(density) { IrisLayout(window.width.toDp(), window.height.toDp()) }
        }
    }
    M3MaterialTheme(colorScheme = IrisM3Colors) {
        TvMaterialTheme(colorScheme = IrisTvColors, typography = IrisTvTypography) {
            CompositionLocalProvider(
                LocalIrisLayout provides layout,
                LocalContentColor provides IrisColor.ink,
                LocalTextStyle provides IrisType.body,
                M3LocalTextStyle provides IrisType.body,
                content = content,
            )
        }
    }
}
