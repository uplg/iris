package studio.kahn.iris.tv.ui.format

import android.graphics.Bitmap
import androidx.core.graphics.get

/**
 * Mean relative luminance of the opaque pixels of a [bitmap], sampled on a [grid]×[grid] grid, within
 * the part given as fractions of its size; null when nothing there is opaque.
 */
fun meanLuminance(bitmap: Bitmap, fromX: Float = 0f, toX: Float = 1f, toY: Float = 1f, grid: Int = 32): Double? {
    val startX = (bitmap.width * fromX).toInt()
    val endX = (bitmap.width * toX).toInt().coerceAtMost(bitmap.width)
    val endY = (bitmap.height * toY).toInt()
    val stepX = maxOf(1, (endX - startX) / grid)
    val stepY = maxOf(1, endY / grid)
    var luma = 0.0
    var count = 0
    var y = 0
    while (y < endY) {
        var x = startX
        while (x < endX) {
            val px = bitmap[x, y]
            if (px ushr 24 and 0xFF > 25) {
                luma += 0.2126 * linear(px ushr 16 and 0xFF) + 0.7152 * linear(px ushr 8 and 0xFF) +
                    0.0722 * linear(px and 0xFF)
                count++
            }
            x += stepX
        }
        y += stepY
    }
    return if (count == 0) null else luma / count
}

private fun linear(c: Int): Double {
    val v = c / 255.0
    return if (v <= 0.04045) v / 12.92 else Math.pow((v + 0.055) / 1.055, 2.4)
}
