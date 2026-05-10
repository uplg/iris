package studio.kahn.iris.tv.ui.theme

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme as TvMaterialTheme
import androidx.tv.material3.Typography as TvTypography
import androidx.tv.material3.darkColorScheme as tvDarkColorScheme
import androidx.compose.material3.MaterialTheme as M3MaterialTheme
import androidx.compose.material3.darkColorScheme as m3DarkColorScheme

// Single source of truth for the palette. Both the TV-Material and the
// stock-Material3 color schemes below pull from these so a label rendered
// inside an `OutlinedTextField` (Material3) reads the same as one inside
// a `Card` (TV-Material).
private val Primary = Color(0xFFC084FC)
private val OnPrimary = Color(0xFF0B0D12)
private val Secondary = Color(0xFFA78BFA)
private val Background = Color(0xFF0B0D12)
private val Surface = Color(0xFF18181B)
private val OnSurface = Color(0xFFF5F5F7)
private val SurfaceVariant = Color(0xFF27272A)
private val OnSurfaceVariant = Color(0xFFA1A1AA)
private val Error = Color(0xFFF87171)

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

// 10-foot typography. The TV-Material defaults are tuned for phones — text
// at 14 sp is unreadable from the couch on a 50" panel. We bump bodyLarge
// to 18 sp and titles up to ~28-32 sp so the UI breathes from typical TV
// viewing distance. Display sizes stay restrained — we don't want a hero
// title that wraps onto three lines on a 720p Mi Box.
@OptIn(ExperimentalTvMaterial3Api::class)
private val IrisTvType = TvTypography(
    displayLarge = TextStyle(fontSize = 56.sp, lineHeight = 64.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-0.5).sp),
    displayMedium = TextStyle(fontSize = 44.sp, lineHeight = 52.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-0.25).sp),
    displaySmall = TextStyle(fontSize = 36.sp, lineHeight = 44.sp, fontWeight = FontWeight.SemiBold),
    headlineLarge = TextStyle(fontSize = 32.sp, lineHeight = 40.sp, fontWeight = FontWeight.SemiBold),
    headlineMedium = TextStyle(fontSize = 28.sp, lineHeight = 36.sp, fontWeight = FontWeight.SemiBold),
    headlineSmall = TextStyle(fontSize = 24.sp, lineHeight = 32.sp, fontWeight = FontWeight.SemiBold),
    titleLarge = TextStyle(fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.SemiBold),
    titleMedium = TextStyle(fontSize = 18.sp, lineHeight = 24.sp, fontWeight = FontWeight.Medium),
    titleSmall = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, fontWeight = FontWeight.Medium),
    bodyLarge = TextStyle(fontSize = 18.sp, lineHeight = 26.sp),
    bodyMedium = TextStyle(fontSize = 16.sp, lineHeight = 22.sp),
    bodySmall = TextStyle(fontSize = 14.sp, lineHeight = 20.sp),
    labelLarge = TextStyle(fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, letterSpacing = 0.5.sp),
    labelMedium = TextStyle(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium, letterSpacing = 0.5.sp),
    labelSmall = TextStyle(fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.Medium, letterSpacing = 0.5.sp),
)

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun IrisTvTheme(content: @Composable () -> Unit) {
    val layout = rememberTvLayout()
    // Provide BOTH theme objects: TV-Material for the focus-aware
    // primitives we use everywhere, stock Material3 so the form widgets
    // and dialog primitives read the same palette. `LocalTvLayout`
    // exposes the responsive sizing struct to every composable so they
    // can branch (or read poster/gutter sizes) by current TV bucket.
    M3MaterialTheme(colorScheme = IrisM3Dark) {
        TvMaterialTheme(colorScheme = IrisTvDark, typography = IrisTvType) {
            CompositionLocalProvider(LocalTvLayout provides layout) {
                content()
            }
        }
    }
}
