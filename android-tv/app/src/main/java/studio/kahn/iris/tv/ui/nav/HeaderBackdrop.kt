package studio.kahn.iris.tv.ui.nav

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import studio.kahn.iris.tv.ui.screens.live.LogoTone
import studio.kahn.iris.tv.ui.screens.live.toneOf
import studio.kahn.iris.tv.ui.theme.IrisColor

/** Words over a still, on their [plate]: a mean tells nothing of a busy picture, no ink alone reads on it. */
@Immutable
data class HeaderInk(val words: Color, val plate: Color)

/**
 * What the header sits on, across the screen: a relative luminance per cell, null where only the
 * ground shows. Each thing in the header reads the part behind it, so the logo on the ground keeps
 * its colours while the name beside it takes a plate over the still.
 */
@Immutable
class HeaderBackdrop(private val cells: List<Double?>) {
    /** How to read between [from] and [to], fractions of the screen; null: as usual. */
    fun inkAt(from: Float, to: Float): HeaderInk? {
        if (cells.isEmpty()) return null
        val first = (from * cells.size).toInt().coerceIn(0, cells.lastIndex)
        val last = (to * cells.size).toInt().coerceIn(first, cells.lastIndex)
        val seen = cells.subList(first, last + 1).filterNotNull()
        val mean = if (seen.isEmpty()) return null else seen.average()
        if (mean < NEAR_GROUND) return null
        return if (toneOf(mean) == LogoTone.Dark) {
            HeaderInk(LogoTone.Dark.plate, IrisColor.artInk.copy(alpha = LIGHT_PLATE_ALPHA))
        } else {
            HeaderInk(LogoTone.Light.plate, IrisColor.stage.copy(alpha = DARK_PLATE_ALPHA))
        }
    }
}

/** Below this a still reads as the ground: the usual words hold, and two tabs side by side stay alike. */
private const val NEAR_GROUND = 0.04

private const val LIGHT_PLATE_ALPHA = 0.85f
private const val DARK_PLATE_ALPHA = 0.75f
