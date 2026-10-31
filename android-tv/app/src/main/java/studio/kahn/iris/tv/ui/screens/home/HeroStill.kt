package studio.kahn.iris.tv.ui.screens.home

import android.graphics.Bitmap
import kotlin.math.pow
import studio.kahn.iris.tv.ui.format.meanLuminance
import studio.kahn.iris.tv.ui.nav.HeaderBackdrop

/** The hero's still: a part of the screen, right-aligned, faded into the ground from its left. */
const val HERO_ART_WIDTH = 1100f / 1920f
const val HERO_ART_HEIGHT = 620f / 1080f
const val FADE_LEFT = 0.45f
const val FADE_BOTTOM = 0.6f

private const val HEADER_CELLS = 48
private const val CELL_GRID = 6
/** The height of the still the header covers. */
private const val HEADER_TO_Y = 0.2f
private const val GROUND_LUMINANCE = 0.01
private const val GAMMA = 2.2

/** The header's backdrop from [art], the luminance of a part of the still given as fractions of its width. */
internal fun heroBackdrop(art: (from: Float, to: Float) -> Double?): HeaderBackdrop {
    val left = 1f - HERO_ART_WIDTH
    return HeaderBackdrop(
        List(HEADER_CELLS) { i ->
            val start = (i.toFloat() / HEADER_CELLS - left) / HERO_ART_WIDTH
            val end = ((i + 1f) / HEADER_CELLS - left) / HERO_ART_WIDTH
            val middle = (start + end) / 2
            val ground = (1f - middle / FADE_LEFT).coerceIn(0f, 1f)
            if (end <= 0f || ground >= 1f) {
                null
            } else {
                art(start.coerceAtLeast(0f), end)?.let { faded(it, ground) }
            }
        },
    )
}

/** [luminance] seen through a fade [ground] opaque: the screen blends gamma-encoded values, not light. */
private fun faded(luminance: Double, ground: Float): Double {
    val encoded = luminance.pow(1 / GAMMA) * (1f - ground) + GROUND_LUMINANCE.pow(1 / GAMMA) * ground
    return encoded.pow(GAMMA)
}

internal fun heroBackdrop(still: Bitmap): HeaderBackdrop =
    heroBackdrop { from, to -> meanLuminance(still, fromX = from, toX = to, toY = HEADER_TO_Y, grid = CELL_GRID) }
