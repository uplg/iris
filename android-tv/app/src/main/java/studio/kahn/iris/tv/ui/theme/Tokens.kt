package studio.kahn.iris.tv.ui.theme

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.floor
import kotlin.math.max

/**
 * Spacing scale, from the gaps the TV boards use (board px / 2 = dp). Use
 * these for padding and gaps; layout-specific sizes live in [IrisSize].
 */
object IrisSpace {
    /** 4 px: a hairline gap. */
    val s0 = 2.dp
    /** 8 px: tabs, a dense row of chips. */
    val s1 = 4.dp
    /** 10-12 px: card text lines, key cap to its words, chip gaps. */
    val s2 = 6.dp
    /** 16 px: action rows, a field's inner gap, fact rows' vertical padding. */
    val s3 = 8.dp
    /** 20 px: a section's title to its content, an icon to its label. */
    val s4 = 10.dp
    /** 24 px: blocks inside a column. */
    val s5 = 12.dp
    /** 28-32 px: the page header's gap, cards in a row. */
    val s6 = 16.dp
    /** 40 px: the page sections, key hints between each other. */
    val s7 = 20.dp
    /** 48 px: two columns side by side. */
    val s8 = 24.dp
    /** 64 px: an aside next to its content. */
    val s9 = 32.dp
    /** 72 px: the keyboard column next to the results. */
    val s10 = 36.dp
}

/** Corner radii by role (board px / 2). */
object IrisRadius {
    /** 8 px: a key cap, a mini poster. */
    val key = 4.dp
    /** 10 px: an on-screen keyboard key. */
    val control = 5.dp
    /** 12 px: posters, stills, rows, fields, side-panel options. */
    val card = 6.dp
    /** 14 px: a large poster beside a page, a framed panel. */
    val panel = 7.dp
    /** Pills, tabs, buttons, meters. */
    val pill = 999.dp
}

/** The shapes behind [IrisRadius], built once. */
object IrisShape {
    val key = RoundedCornerShape(IrisRadius.key)
    val control = RoundedCornerShape(IrisRadius.control)
    val card = RoundedCornerShape(IrisRadius.card)
    val panel = RoundedCornerShape(IrisRadius.panel)
    val pill = RoundedCornerShape(IrisRadius.pill)
    val circle = CircleShape
}

/**
 * The one focus treatment (every board): a 4 px accent outline 4 px outside
 * the element; a focused card grows to 1.06; a focused button, pill, tab or
 * row fills with [IrisColor.ink] and its words turn [IrisColor.ground].
 */
object IrisFocus {
    val ringWidth = 2.dp
    val ringOffset = 2.dp
    const val cardScale = 1.06f
    const val animationMs = 150
}

/** Component sizes from the boards (px / 2). */
object IrisSize {
    /** 72 px: a large action, the search field, a getting-ready step. */
    val controlLarge = 36.dp
    /** 64 px: a regular action, an on-screen key. */
    val control = 32.dp
    /** 56 px: a tab, a small action. */
    val controlSmall = 28.dp
    /** 52 px: a filter pill. */
    val pill = 26.dp
    /** 40 px: a chip. */
    val chip = 20.dp
    /** 36 px: a small chip. */
    val chipSmall = 18.dp
    /** 68 px: a side-panel option. */
    val option = 34.dp
    /** 44 px: the avatar disc and the logo mark. */
    val avatar = 22.dp
    val mark = 22.dp
    /** 28 px: an icon beside an action's label. */
    val icon = 14.dp
    /** 16 px: a status icon beside meta words. */
    val iconSmall = 9.dp
    /** 36 px tall, at least 44 px wide: a key cap. */
    val keyCapHeight = 18.dp
    val keyCapMinWidth = 22.dp
    /** 8 px: a meter's track. */
    val meter = 4.dp
    /** 200 px: a poster in a row. */
    val posterRow = 100.dp
    /** The narrowest poster in a grid (7 columns on a TV). */
    val posterGridMin = 104.dp
    /** The narrowest poster in a dense grid (8 columns on a TV). */
    val posterDenseMin = 92.dp
    /** 384 px: a 16:9 still in a row (continue watching). */
    val stillRow = 192.dp
    /** 72 px: a poster beside a release row. */
    val posterMini = 36.dp
    /** 300 px: the poster in a page's aside. */
    val posterAside = 150.dp
    /** The poster inside a title's banner, and on a short screen. */
    val posterBanner = 112.dp
    val posterBannerCompact = 72.dp
    /** The aside's poster on a short or narrow screen (a landscape phone). */
    val posterAsideCompact = 104.dp
    /** 420 px: a page's aside when it carries words beside the poster. */
    val asideColumn = 210.dp
    /** 920 px: a centered dialog or a card over the stage. */
    val dialog = 460.dp
    /** 680 px: a side panel. */
    val sidePanel = 340.dp
    /** 200 px: a fact's label column. */
    val factLabel = 100.dp
    /** 44 px: a step's icon column. */
    val stepIcon = 22.dp
}

/** The bars over the picture (the player's, a channel's: TVPlayer). */
object IrisStageBar {
    /** 28 px: between a bar's lines, and under the top bar's. */
    val gap = 14.dp
    /** 32 px: above the bottom bar's first line. */
    val top = 16.dp
    /** 46 px: under the bottom bar's key hints. */
    val bottom = 23.dp
}

/**
 * The screen the app draws on, and what follows from it: the 5 % safe
 * margins (96x54 px on the boards = 48x27 dp on a TV, less on a phone) and
 * grid columns computed from the width left between them. The same APK runs
 * on landscape phones (800x360, 915x412 dp), so never hard-code a TV count.
 *
 * Read it with [IrisLayout.current]; [IrisTheme] provides it from the window.
 */
@Immutable
data class IrisLayout(val width: Dp, val height: Dp) {
    val safeHorizontal: Dp get() = width * SAFE_FRACTION
    val safeVertical: Dp get() = height * SAFE_FRACTION
    /** Padding that keeps content inside the TV's overscan-safe area. */
    val safePadding: PaddingValues
        get() = PaddingValues(horizontal = safeHorizontal, vertical = safeVertical)
    /** Width between the safe margins. */
    val contentWidth: Dp get() = width - safeHorizontal * 2

    /** A landscape phone's width: an aside and its content stack tighter. */
    val narrow: Boolean get() = width < NARROW_BELOW
    /** A landscape phone's height: a page drops what does not fit in a short screen. */
    val short: Boolean get() = height < SHORT_BELOW

    /** How many cells at least [minCell] wide fit [available] with [gap] between them. */
    fun columns(minCell: Dp, gap: Dp, available: Dp = contentWidth): Int =
        columnsFor(available.value, minCell.value, gap.value)

    companion object {
        const val SAFE_FRACTION = 0.05f
        val NARROW_BELOW = 900.dp
        val SHORT_BELOW = 500.dp
        /** A 1080p or 720p TV: both are 960x540 dp. */
        val Tv = IrisLayout(960.dp, 540.dp)

        val current: IrisLayout
            @Composable @ReadOnlyComposable get() = LocalIrisLayout.current
    }
}

/** Pure column count, shared with the unit tests. */
fun columnsFor(available: Float, minCell: Float, gap: Float): Int =
    max(1, floor((available + gap) / (minCell + gap)).toInt())

val LocalIrisLayout = staticCompositionLocalOf { IrisLayout.Tv }
