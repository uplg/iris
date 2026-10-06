package studio.kahn.iris.tv.ui.screens.live

import android.graphics.Bitmap
import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.core.graphics.get
import java.time.OffsetDateTime
import java.util.concurrent.ConcurrentHashMap
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveProgramme
import studio.kahn.iris.tv.ui.theme.IrisColor

/** 0..1 position of [nowMs] inside a programme, null outside its window (the web's `programmeProgress`). */
fun programmeProgress(start: OffsetDateTime, stop: OffsetDateTime, nowMs: Long): Float? {
    val s = start.toInstant().toEpochMilli()
    val e = stop.toInstant().toEpochMilli()
    if (e <= s) return null
    val pos = (nowMs - s).toFloat() / (e - s)
    return pos.takeIf { it in 0f..1f }
}

/** "Now: News, until 21:00". [clock] formats a time the viewer's way. */
fun nowWords(p: LiveProgramme, clock: (OffsetDateTime) -> String): String {
    val until = clock(p.stop)
    return if (until.isNotEmpty()) "Now: ${p.title}, until $until" else "Now: ${p.title}"
}

/** "Next at 21:00: Film". */
fun nextWords(p: LiveProgramme, clock: (OffsetDateTime) -> String): String {
    val at = clock(p.start)
    return if (at.isNotEmpty()) "Next at $at: ${p.title}" else "Next: ${p.title}"
}

/** A country's channels, the web's order: the numbered TNT ones first, then by first category. */
@Immutable
data class ChannelSection(val title: String, val channels: List<LiveChannel>)

fun channelSections(channels: List<LiveChannel>): List<ChannelSection> {
    val tnt = channels.filter { it.tntNumber != null }
    val byCategory = LinkedHashMap<String, MutableList<LiveChannel>>()
    channels.filter { it.tntNumber == null }.forEach { ch ->
        byCategory.getOrPut(ch.categories.firstOrNull() ?: "Other") { mutableListOf() }.add(ch)
    }
    return buildList {
        if (tnt.isNotEmpty()) add(ChannelSection("TNT", tnt))
        byCategory.forEach { (title, list) -> add(ChannelSection(title, list)) }
    }
}

/** A server-relative path (`/api/livetv/logo?…`) made absolute against the Iris base URL. */
fun absolutize(base: String, path: String?): String? {
    if (path == null || !path.startsWith('/')) return path
    return base.trimEnd('/') + path
}

/** The plate behind a channel logo, from the logo's own luminance (web `logo-tone.ts`). */
enum class LogoTone(val plate: Color) {
    /** A dark logo: a light plate. */
    Light(IrisColor.artInk),
    Neutral(IrisColor.inkMuted),
    /** A light logo: a dark plate. */
    Dark(IrisColor.stage),
}

/** One luminance pass per logo: the grid recomposes on every guide refresh. */
val logoToneCache = ConcurrentHashMap<String, LogoTone>()

/** Mean luminance of the opaque pixels, sampled on a 32×32 grid; the web's thresholds. */
fun logoTone(bitmap: Bitmap): LogoTone {
    val stepX = maxOf(1, bitmap.width / 32)
    val stepY = maxOf(1, bitmap.height / 32)
    var luma = 0.0
    var count = 0
    var y = 0
    while (y < bitmap.height) {
        var x = 0
        while (x < bitmap.width) {
            val px = bitmap[x, y]
            if (px ushr 24 and 0xFF > 25) {
                luma += 0.2126 * (px ushr 16 and 0xFF) + 0.7152 * (px ushr 8 and 0xFF) + 0.0722 * (px and 0xFF)
                count++
            }
            x += stepX
        }
        y += stepY
    }
    return toneOf(if (count == 0) null else luma / count / 255.0)
}

/** The plate for a mean luminance (0..1), null = nothing opaque. */
fun toneOf(mean: Double?): LogoTone = when {
    mean == null -> LogoTone.Neutral
    mean < 0.38 -> LogoTone.Light
    mean > 0.62 -> LogoTone.Dark
    else -> LogoTone.Neutral
}
