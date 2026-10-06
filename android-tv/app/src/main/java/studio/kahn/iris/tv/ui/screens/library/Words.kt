package studio.kahn.iris.tv.ui.screens.library

import java.time.Duration
import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.roundToLong
import studio.kahn.iris.tv.data.VIDEO_EXTENSIONS

// The web's `@iris/api/format` words, said the same way on the TV.

/** A count with its noun: `1 release`, `3 releases`. */
fun plural(n: Number, one: String, many: String = "${one}s"): String =
    "$n ${if (n.toLong() == 1L) one else many}"

/** A share as people read it: `42%`. */
fun percent(fraction0to100: Double): String = "${Math.round(fraction0to100)}%"

/** A length as people say it: `55 min`, `1 h 12 min`, `45 s` under a minute. */
fun duration(seconds: Double): String {
    val total = max(0L, seconds.roundToLong())
    if (total < 60) return "$total s"
    val h = total / 3600
    val m = ((total % 3600) / 60.0).roundToLong()
    if (h == 0L) return "$m min"
    return if (m == 0L) "$h h" else "$h h $m min"
}

/** What is left to watch: `23 min left`. */
fun timeLeft(seconds: Double): String = "${duration(seconds)} left"

/** A position as a clock: `32:10`, `1:02:03`; `--:--` when unknown. */
fun clock(seconds: Double?): String {
    if (seconds == null || !seconds.isFinite() || seconds < 0) return "--:--"
    val s = floor(seconds).toLong()
    val h = s / 3600
    val m = (s % 3600) / 60
    val sec = s % 60
    return if (h > 0) "%d:%02d:%02d".format(Locale.ROOT, h, m, sec) else "%d:%02d".format(Locale.ROOT, m, sec)
}

/** An episode's code: `S2:E4`; a season alone: `Season 2`. */
fun episodeCode(season: Long?, episode: Long?): String? {
    if (season == null) return episode?.let { "E$it" }
    if (episode == null || episode == 0L) return "Season $season"
    return "S$season:E$episode"
}

/** A release name without its video extension (a single-file torrent is named after its file). */
fun releaseName(name: String): String {
    val ext = VIDEO_EXTENSIONS.firstOrNull { name.endsWith(it, ignoreCase = true) } ?: return name
    return name.dropLast(ext.length)
}

/** A file's own name, without its folders. */
fun fileName(path: String?): String? = path?.substringAfterLast('/')

private val SCENE_STOP = Regex(
    "^(19\\d{2}|20\\d{2}|s\\d{1,2}(e\\d{1,3})?|e\\d{1,3}|\\d{3,4}p|web|web-?dl|webrip|bluray|blu-?ray|brrip|bdrip|hdtv|" +
        "dvdrip|dvd|remux|x264|x265|h264|h265|hevc|avc|xvid|divx|aac\\d?|ac3|eac3|dts(-?hd)?(-?ma)?|ddp?\\d?|truehd|" +
        "atmos|flac|multi|vff|vfi|vof|vfq|vostfr|vost|vo|vf|french|truefrench|english|hdr|hdr10\\+?|dovi|dv|10bit|" +
        "8bit|repack|proper|internal|limited|uncut|unrated|extended|imax|complete|integrale)$",
    RegexOption.IGNORE_CASE,
)
private val SCENE_EXT = Regex("\\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv|srt|nfo)$", RegexOption.IGNORE_CASE)

/** A raw SCENE name made readable when no verified title is known: `Mercato (2025)`. */
fun prettySceneName(raw: String): String {
    val tokens = raw.replace(SCENE_EXT, "").split(Regex("[._\\s]+")).filter { it.isNotEmpty() }
    val title = mutableListOf<String>()
    var year: String? = null
    for (t in tokens) {
        if (SCENE_STOP.matches(t)) {
            if (Regex("^(19|20)\\d{2}$").matches(t) && title.isNotEmpty()) year = t
            break
        }
        title += t
    }
    if (title.isEmpty()) return tokens.joinToString(" ")
    val name = title.joinToString(" ")
    return if (year != null) "$name ($year)" else name
}

/** `just now`, `5m ago`, `3h ago`, `2d ago`. */
fun recentTime(at: OffsetDateTime, now: Instant = Instant.now()): String {
    val secs = max(0L, Duration.between(at.toInstant(), now).seconds)
    return when {
        secs < 10 -> "just now"
        secs < 60 -> "${secs}s ago"
        secs < 3600 -> "${secs / 60}m ago"
        secs < 86_400 -> "${secs / 3600}h ago"
        else -> "${secs / 86_400}d ago"
    }
}

private val TIME = DateTimeFormatter.ofPattern("HH:mm", Locale.UK)
private val DAY_MONTH = DateTimeFormatter.ofPattern("d MMM", Locale.UK)
private val DAY_MONTH_YEAR = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.UK)

/** A moment as people say it: `today at 21:04`, `yesterday at 09:12`, `on Monday`, `on 2 Oct`. */
fun onDay(at: OffsetDateTime, now: Instant = Instant.now(), zone: ZoneId = ZoneId.systemDefault()): String {
    val local = at.atZoneSameInstant(zone)
    val today = now.atZone(zone).toLocalDate()
    val days = ChronoUnit.DAYS.between(today, local.toLocalDate())
    return when {
        days == 0L -> "today at ${local.format(TIME)}"
        days == -1L -> "yesterday at ${local.format(TIME)}"
        days == 1L -> "tomorrow at ${local.format(TIME)}"
        abs(days) < 7 -> "on ${local.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.UK)}"
        local.year == today.year -> "on ${local.format(DAY_MONTH)}"
        else -> "on ${local.format(DAY_MONTH_YEAR)}"
    }
}

private val ISO3: Map<String, String> by lazy {
    Locale.getISOLanguages().associateBy { runCatching { Locale.forLanguageTag(it).isO3Language }.getOrDefault(it) }
}

// ISO 639-2/B codes, which Locale does not know (player tracks carry them: `fre`, `ger`).
private val BIBLIOGRAPHIC = mapOf(
    "fre" to "fr", "ger" to "de", "dut" to "nl", "chi" to "zh", "cze" to "cs", "gre" to "el", "per" to "fa",
    "rum" to "ro", "slo" to "sk", "alb" to "sq", "arm" to "hy", "baq" to "eu", "geo" to "ka", "ice" to "is",
    "mac" to "mk", "may" to "ms", "wel" to "cy",
)

/** A language code (`en`, `fra`, `fre`) as its English name; the code itself when unknown. */
fun languageName(code: String?): String? {
    val c = code?.trim()?.lowercase()?.takeIf { it.isNotEmpty() } ?: return null
    val two = if (c.length == 3) BIBLIOGRAPHIC[c] ?: ISO3[c] ?: c else c
    val name = Locale.forLanguageTag(two).getDisplayLanguage(Locale.ENGLISH)
    return name.takeIf { it.isNotBlank() && !it.equals(c, ignoreCase = true) } ?: c.uppercase()
}
