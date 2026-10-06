@file:Suppress("unused")

package studio.kahn.iris.tv.ui.theme

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

// Pre-redesign names mapped onto the synthe.se tokens so the screens keep
// compiling until each is rewritten. DELETE this file once no screen uses it.

@Deprecated("redesign: use IrisTheme tokens (IrisColor)")
object IrisColors {
    val Background = IrisColor.ground
    val BackgroundDeep = IrisColor.ground
    val Foreground = IrisColor.ink
    val Card = IrisColor.surface
    val Elev2 = IrisColor.groundRaised
    val MutedForeground = IrisColor.inkMuted
    val FgDim = IrisColor.inkMuted
    val Border = IrisColor.line
    val BorderStrong = IrisColor.line
    val Overlay06 = IrisColor.surface
    val Overlay12 = IrisColor.groundRaised
    val Brand = IrisColor.accent
    val BrandHi = IrisColor.accent
    val Brand2 = IrisColor.accent
    val Brand3 = IrisColor.accent
    val OnBrand = IrisColor.onAccent
    val BrandSoft = IrisColor.accentWash
    val BrandGlow: Color = IrisColor.accentWash
    val Success = IrisColor.accent
    val Warn = IrisColor.warn
    val Destructive = IrisColor.down
}

@Deprecated("redesign: use IrisTheme tokens (IrisSpace, IrisLayout.safeHorizontal)")
object Spacing {
    val xs = IrisSpace.s1
    val sm = IrisSpace.s3
    val md = IrisSpace.s5
    val lg = IrisSpace.s6
    val xl = IrisSpace.s8
    val xxl = IrisSpace.s9
    val xxxl = 48.dp
    val gutter = 48.dp
}

@Deprecated("redesign: use IrisTheme tokens (IrisRadius, IrisShape)")
object Radius {
    val sm = IrisRadius.key
    val md = IrisRadius.card
    val lg = IrisRadius.panel
    val xl = IrisRadius.panel
    val poster = IrisRadius.card
    val button = IrisRadius.pill
    val panel = IrisRadius.panel
    val pill = IrisRadius.pill
}

@Deprecated("redesign: use IrisTheme tokens (IrisFocus)")
object Focus {
    val ring = IrisFocus.ringWidth
    const val posterScale = IrisFocus.cardScale
    const val controlScale = 1f
    val glow = 0.dp
}

@Deprecated("redesign: use IrisTheme tokens (IrisSize)")
object CardSize {
    val sm = IrisSize.posterRow
    val md = IrisSize.posterAside
    val lg = IrisSize.stillRow
}

@Deprecated("redesign: use IrisLayout.current")
data class TvLayout(
    val gutterHorizontal: Dp,
    val gutterVertical: Dp,
    val gridPosterMin: Dp,
    val shelfPosterWidth: Dp,
    val heroAspect: Float,
    val detailRail: Dp,
)

@Suppress("DEPRECATION")
private val LegacyLayout = TvLayout(
    gutterHorizontal = 48.dp,
    gutterVertical = 27.dp,
    gridPosterMin = IrisSize.posterGridMin,
    shelfPosterWidth = IrisSize.posterRow,
    heroAspect = 16f / 6f,
    detailRail = 280.dp,
)

@Suppress("DEPRECATION")
@Deprecated("redesign: use IrisLayout.current")
object LocalTvLayout {
    val current: TvLayout get() = LegacyLayout
}

@Deprecated("redesign: use FontText (Cal Sans 2)")
val Inter: FontFamily = CalSans

@Deprecated("redesign: use FontText (Cal Sans 2)")
val FontSans: FontFamily = CalSans

@Deprecated("redesign: use FontMono")
val JetBrainsMono: FontFamily = FontFamily.Monospace
