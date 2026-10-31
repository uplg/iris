package studio.kahn.iris.tv.ui.nav

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import studio.kahn.iris.tv.ui.screens.live.toneOf

/**
 * What the header sits on, across the screen: a relative luminance per cell, null where only the
 * ground shows. Each thing in the header reads the part behind it, so the logo on the ground keeps
 * its colours while the name beside it goes dark over a bright still.
 */
@Immutable
class HeaderBackdrop(private val cells: List<Double?>) {
    /** The words that read best between [from] and [to], fractions of the screen; null: the usual ones. */
    fun inkAt(from: Float, to: Float): Color? {
        if (cells.isEmpty()) return null
        val first = (from * cells.size).toInt().coerceIn(0, cells.lastIndex)
        val last = (to * cells.size).toInt().coerceIn(first, cells.lastIndex)
        val seen = cells.subList(first, last + 1).filterNotNull()
        val mean = if (seen.isEmpty()) return null else seen.average()
        return if (mean < NEAR_GROUND) null else toneOf(mean).plate
    }
}

/** Below this a still reads as the ground: the usual words hold, and two tabs side by side stay alike. */
private const val NEAR_GROUND = 0.04
