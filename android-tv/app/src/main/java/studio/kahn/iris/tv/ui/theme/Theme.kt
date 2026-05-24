package studio.kahn.iris.tv.ui.theme

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme as TvMaterialTheme
import androidx.tv.material3.darkColorScheme as tvDarkColorScheme
import androidx.compose.material3.MaterialTheme as M3MaterialTheme
import androidx.compose.material3.darkColorScheme as m3DarkColorScheme

// Single source of truth for the palette: the OKLCH→sRGB tokens in
// `IrisColors` (Color.kt), ported from the web design system. Both the
// TV-Material and the stock-Material3 color schemes below pull from these so a
// label rendered inside an `OutlinedTextField` (Material3) reads the same as
// one inside a `Card` (TV-Material).
private val Primary = IrisColors.Brand
private val OnPrimary = IrisColors.OnBrand
private val Secondary = IrisColors.Brand2
private val Background = IrisColors.Background
private val Surface = IrisColors.Card
private val OnSurface = IrisColors.Foreground
private val SurfaceVariant = IrisColors.Elev2
private val OnSurfaceVariant = IrisColors.MutedForeground
private val Error = IrisColors.Destructive

private val IrisTvDark = tvDarkColorScheme(
    primary = Primary,
    onPrimary = OnPrimary,
    secondary = Secondary,
    background = Background,
    surface = Surface,
    onSurface = OnSurface,
    onBackground = OnSurface,
    surfaceVariant = SurfaceVariant,
    onSurfaceVariant = OnSurfaceVariant,
)

// The stock Material3 dark scheme. We need this in addition to the TV one
// because composables that ship in `androidx.compose.material3`
// (`OutlinedTextField`, `AlertDialog`, `TextButton`, `LinearProgressIndicator`
// …) read their colors from `androidx.compose.material3.MaterialTheme`,
// not from the TV theme. Without this bridge, every Material3 surface
// fell back to the default LIGHT palette, which is what produced the
// "black text on a dark background" complaint.
private val IrisM3Dark = m3DarkColorScheme(
    primary = Primary,
    onPrimary = OnPrimary,
    secondary = Secondary,
    background = Background,
    surface = Surface,
    onSurface = OnSurface,
    onBackground = OnSurface,
    surfaceVariant = SurfaceVariant,
    onSurfaceVariant = OnSurfaceVariant,
    error = Error,
)

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun IrisTvTheme(content: @Composable () -> Unit) {
    val layout = rememberTvLayout()
    // Provide BOTH theme objects: TV-Material for the focus-aware
    // primitives we use everywhere, stock Material3 so the form widgets
    // and dialog primitives read the same palette. Typography
    // (Cal Sans / Inter / JetBrains Mono) lives in `Type.kt`. `LocalTvLayout`
    // exposes the responsive sizing struct to every composable so they
    // can branch (or read poster/gutter sizes) by current TV bucket.
    M3MaterialTheme(colorScheme = IrisM3Dark) {
        TvMaterialTheme(colorScheme = IrisTvDark, typography = IrisTvTypography) {
            CompositionLocalProvider(LocalTvLayout provides layout) {
                content()
            }
        }
    }
}
