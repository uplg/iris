package studio.kahn.iris.tv.ui.screens.live

import android.graphics.Bitmap
import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.core.graphics.get
import java.text.Normalizer
import java.time.OffsetDateTime
import java.util.concurrent.ConcurrentHashMap
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveCountry
import studio.kahn.iris.tv.data.LiveProgramme
import studio.kahn.iris.tv.ui.format.plural
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

/** A section of a country's channels; [key] is the category filter's value (web `guide.ts`). */
@Immutable
data class ChannelSection(val key: String, val title: String, val channels: List<LiveChannel>)

const val TNT_SECTION = "tnt"
private const val OTHER = "Other"

/**
 * The French free-to-air channels by their number, then one section per first category
 * (alphabetical, "Other" last). Within a category, a channel that will likely not play goes
 * to the end; the free-to-air ones keep their numbers' order.
 */
fun channelSections(channels: List<LiveChannel>): List<ChannelSection> {
    val tnt = channels.filter { it.tntNumber != null }.sortedBy { it.tntNumber }
    val byCategory = LinkedHashMap<String, MutableList<LiveChannel>>()
    channels.filter { it.tntNumber == null }.forEach { ch ->
        byCategory.getOrPut(ch.categories.firstOrNull() ?: OTHER) { mutableListOf() }.add(ch)
    }
    val titles = byCategory.keys.sortedWith(compareBy<String> { it == OTHER }.thenBy { it })
    return buildList {
        if (tnt.isNotEmpty()) add(ChannelSection(TNT_SECTION, "Free-to-air (TNT)", tnt))
        titles.forEach { title ->
            add(ChannelSection("cat:$title", title, byCategory.getValue(title).sortedBy { if (dimmed(it)) 1 else 0 }))
        }
    }
}

/** A channel whose every feed is DRM-locked with no licence Iris can obtain. */
const val ENCRYPTED_WORDS = "Encrypted by the broadcaster: it can't be played here."

/** Why a channel may not play, in words; null when nothing is known against it. */
fun channelNotice(c: LiveChannel): String? = when {
    c.encrypted == true -> ENCRYPTED_WORDS
    c.unreachable == true -> "Not answering right now"
    c.geoBlocked -> "May be blocked in your country"
    c.not247 -> "Not on air all day"
    else -> null
}

/** Said less loudly: a channel that will likely not play. */
fun dimmed(c: LiveChannel): Boolean = c.encrypted == true || c.unreachable == true || c.geoBlocked

/** How many picked countries the TV keeps. */
const val RECENT_COUNTRIES = 3

/** [code] picked: first of the recent ones, the oldest falling off. */
fun rememberCountry(recent: List<String>, code: String): List<String> =
    (listOf(code) + recent.filter { it != code }).take(RECENT_COUNTRIES)

/** The household's usual countries: the server's default first, then the ones last picked (only those still offered). */
fun usualCountries(defaultCode: String?, recent: List<String>, offered: List<LiveCountry>): List<LiveCountry> {
    val byCode = offered.associateBy { it.code }
    return (listOfNotNull(defaultCode) + recent).distinct().mapNotNull { byCode[it] }
}

private fun fold(s: String): String =
    Normalizer.normalize(s, Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "").lowercase().trim()

/** The countries a typed text names: a name starting with it first, then one containing it, or the code itself. */
fun findCountries(countries: List<LiveCountry>, text: String): List<LiveCountry> {
    val q = fold(text)
    if (q.isEmpty()) return countries
    val (starts, rest) = countries.partition { fold(it.name).startsWith(q) || it.code == q }
    return starts + rest.filter { fold(it.name).contains(q) }
}

/** "France · 42 channels", or the name alone when the server does not know yet. */
fun countryLabel(c: LiveCountry): String =
    listOfNotNull("${c.flag} ${c.name}", c.channelCount?.let { plural(it, "channel") }).joinToString(" · ")

/** A server-relative path (`/api/livetv/logo?…`) made absolute against the Iris base URL. */
fun absolutize(base: String, path: String?): String? {
    if (path == null || !path.startsWith('/')) return path
    return base.trimEnd('/') + path
}

/** The plate behind a channel logo: whichever of light and dark contrasts more with the logo's
 *  mean luminance (web `logo-tone.ts`). Neutral only when nothing could be read. */
enum class LogoTone(val plate: Color) {
    /** A dark logo: a light plate. */
    Light(IrisColor.artInk),
    Neutral(IrisColor.inkMuted),
    /** A light logo: a dark plate. */
    Dark(IrisColor.stage),
}

/** One luminance pass per logo: the grid recomposes on every guide refresh. */
val logoToneCache = ConcurrentHashMap<String, LogoTone>()

/** Mean relative luminance of the opaque pixels, sampled on a 32×32 grid. */
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
                luma += 0.2126 * linear(px ushr 16 and 0xFF) + 0.7152 * linear(px ushr 8 and 0xFF) +
                    0.0722 * linear(px and 0xFF)
                count++
            }
            x += stepX
        }
        y += stepY
    }
    return toneOf(if (count == 0) null else luma / count)
}

private fun linear(c: Int): Double {
    val v = c / 255.0
    return if (v <= 0.04045) v / 12.92 else Math.pow((v + 0.055) / 1.055, 2.4)
}

/** Relative luminance of the light and dark plates. */
private const val LIGHT_PLATE = 0.92
private const val DARK_PLATE = 0.012

/** The plate for a mean relative luminance (0..1) — the higher WCAG contrast; null = nothing
 *  opaque. No grey in between: a red, orange or grey logo all but vanished on it. */
fun toneOf(mean: Double?): LogoTone = when {
    mean == null -> LogoTone.Neutral
    (LIGHT_PLATE + 0.05) / (mean + 0.05) >= (mean + 0.05) / (DARK_PLATE + 0.05) -> LogoTone.Light
    else -> LogoTone.Dark
}
