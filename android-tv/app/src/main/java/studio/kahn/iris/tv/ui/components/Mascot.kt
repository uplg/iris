package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.Dp
import studio.kahn.iris.tv.ui.theme.IrisColor

private val ANTENNAS = listOf("M19 8.5l5 6 5-6", "M24 14.5V9.5M24 6.5v.01", "M18 14.5l-2-6M30 14.5l2-6", "M24 14.5l5-6 4 2")

private val EYES = listOf(
    "M16.4 25.4a2.3 2.3 0 0 1 4.4 0M27.2 25.4a2.3 2.3 0 0 1 4.4 0",
    "M16.4 24a2.3 2.3 0 0 0 4.4 0M27.2 24a2.3 2.3 0 0 0 4.4 0",
    "M18.6 23.2v2.8M29.4 23.2v2.8",
    "M16.4 25.4a2.3 2.3 0 0 1 4.4 0M29.4 23.2v2.8",
    "M16.2 24.8h4.8M27 24.8h4.8",
    "M16.6 23.4l4 1.6M31.4 23.4l-4 1.6",
)

private val SCREENS = listOf(
    "M15.5 15h17a6 6 0 0 1 6 6v10a6 6 0 0 1-6 6h-17a6 6 0 0 1-6-6v-10a6 6 0 0 1 6-6z",
    "M12.5 15h23a3 3 0 0 1 3 3v16a3 3 0 0 1-3 3h-23a3 3 0 0 1-3-3v-16a3 3 0 0 1 3-3z",
    "M19.5 15h9a10 10 0 0 1 10 10v2a10 10 0 0 1-10 10h-9a10 10 0 0 1-10-10v-2a10 10 0 0 1 10-10z",
)

private val FEET = listOf("M19 41h10", "M15 38v3M33 38v3", "M14 41h20")

private const val BLUSH = "M13.2 29.6a2 1.2 0 1 0 4 0a2 1.2 0 1 0-4 0zM30.8 29.6a2 1.2 0 1 0 4 0a2 1.2 0 1 0-4 0z"

private const val GRID = 48f
private const val TILE_CORNER = 11f
private const val LINE = 2.4f
private const val EYE_LINE = 1.7f

/** Which of each part a name draws: indexes into the tables above, the same as the web's. */
@Immutable
data class MascotParts(val tile: Int, val antenna: Int, val eyes: Int, val screen: Int, val foot: Int)

/** FNV-1a over the UTF-8 of the name, trimmed and in lower case (web `mascotHash`). */
fun mascotHash(name: String): Long {
    var h = 0x811c9dc5L
    for (byte in name.trim().lowercase().toByteArray()) {
        h = ((h xor (byte.toLong() and 0xFF)) * 0x01000193L) and 0xFFFFFFFFL
    }
    return h
}

/** A person's mascot (web `@iris/api/mascot`): `MascotTest` and the web's test hold the same vectors. */
fun mascotOf(name: String): MascotParts {
    var h = mascotHash(name)
    fun take(n: Int): Int = (h % n).toInt().also { h /= n }
    return MascotParts(take(IrisColor.mascots.size), take(ANTENNAS.size), take(EYES.size), take(SCREENS.size), take(FEET.size))
}

private fun path(d: String): Path = PathParser().parsePathString(d).toPath()

/** A person's mascot, the Iris mark's grammar: decoration beside their name, which always says it. */
@Composable
fun Mascot(name: String, size: Dp, modifier: Modifier = Modifier) {
    val parts = remember(name) { mascotOf(name) }
    val shapes = remember(parts) {
        listOf(ANTENNAS[parts.antenna], FEET[parts.foot], SCREENS[parts.screen], BLUSH, EYES[parts.eyes]).map(::path)
    }
    val (antenna, foot, screen, blush, eyes) = shapes
    val tile = IrisColor.mascots[parts.tile]
    Canvas(modifier.size(size)) {
        scale(this.size.width / GRID, pivot = Offset.Zero) {
            val line = Stroke(LINE, cap = StrokeCap.Round, join = StrokeJoin.Round)
            drawRoundRect(tile, size = Size(GRID, GRID), cornerRadius = CornerRadius(TILE_CORNER))
            drawPath(antenna, IrisColor.artInk, style = line)
            drawPath(screen, IrisColor.artInk)
            drawPath(eyes, tile, style = Stroke(EYE_LINE, cap = StrokeCap.Round, join = StrokeJoin.Round))
            drawPath(blush, IrisColor.mascotBlush)
            drawPath(foot, IrisColor.artInk, style = line)
        }
    }
}
