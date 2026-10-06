package studio.kahn.iris.tv.ui.format

import java.time.ZoneId
import java.time.Duration
import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.roundToLong

/** A playback position or length as a clock: `32:10`, `1:02:03`; `--:--` when unknown. */
fun clock(seconds: Double?): String {
    if (seconds == null || !seconds.isFinite() || seconds < 0) return "--:--"
    val s = floor(seconds).toLong()
    val h = s / 3600
    val m = (s % 3600) / 60
    val sec = s % 60
    return if (h > 0) "%d:%02d:%02d".format(Locale.ROOT, h, m, sec) else "%d:%02d".format(Locale.ROOT, m, sec)
}

/** A length as people say it: `55 min`, `1 h 12 min`, `45 s` under a minute. */
fun duration(seconds: Double): String {
    val total = max(0L, seconds.roundToLong())
    if (total < 60) return "$total s"
    // The minutes rounded first, so 59 min 30 s is "1 h", never "60 min" (the web's rule).
    val minutes = (total / 60.0).roundToLong()
    val h = minutes / 60
    val m = minutes % 60
    if (h == 0L) return "$m min"
    return if (m == 0L) "$h h" else "$h h $m min"
}

/** How long until a download is done: `done in about 6 min`. */
fun etaWords(seconds: Double): String = "done in about ${duration(seconds)}"

/** What is left to watch: `23 min left`. */
fun timeLeft(seconds: Double): String = "${duration(seconds)} left"

private val TIME = DateTimeFormatter.ofPattern("HH:mm", Locale.UK)
private val DAY_MONTH = DateTimeFormatter.ofPattern("d MMM", Locale.UK)

/** A time of day, `21:05`: 24 h whatever the device says, as every time the app says (web `clockTime`). */
fun clockTime(at: OffsetDateTime, zone: ZoneId = ZoneId.systemDefault()): String = at.atZoneSameInstant(zone).format(TIME)

fun clockTime(at: Instant, zone: ZoneId = ZoneId.systemDefault()): String = at.atZone(zone).format(TIME)
private val DAY_MONTH_YEAR = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.UK)

/** Whether a past moment follows a verb (`Added yesterday at 21:04`) or stands in a line of facts (`yesterday 21:04`). */
enum class AgoStyle { Sentence, Short }

/**
 * A moment's day, past or future, in [now]'s zone (web `dayWords`): `today at 21:04`,
 * `yesterday at 09:12`, `tomorrow at 08:00`, `on Monday`, `on 2 Oct`, `on 2 Oct 2025`;
 * [short] drops the "at" and the "on" (`yesterday 21:04`, `Monday`, `2 Oct`).
 */
private fun dayWords(at: OffsetDateTime, now: ZonedDateTime, short: Boolean): String {
    val local = at.atZoneSameInstant(now.zone)
    val days = ChronoUnit.DAYS.between(now.toLocalDate(), local.toLocalDate())
    val time = if (short) " ${local.format(TIME)}" else " at ${local.format(TIME)}"
    val on = if (short) "" else "on "
    return when {
        days == 0L -> "today$time"
        days == -1L -> "yesterday$time"
        days == 1L -> "tomorrow$time"
        abs(days) < 7 -> on + local.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.UK)
        local.year == now.year -> on + local.format(DAY_MONTH)
        else -> on + local.format(DAY_MONTH_YEAR)
    }
}

/** A moment in a sentence, mostly an upcoming one: `Signed in until tomorrow at 08:00`. A past one is [ago]. */
fun onDay(at: OffsetDateTime, now: ZonedDateTime = ZonedDateTime.now()): String = dayWords(at, now, short = false)

/**
 * A past moment, the one way the app says it (web `ago`): to the minute within the hour
 * (`just now`, `12 min ago`), then by its day ([AgoStyle]).
 */
fun ago(at: OffsetDateTime, style: AgoStyle = AgoStyle.Sentence, now: ZonedDateTime = ZonedDateTime.now()): String {
    val secs = Duration.between(at.toInstant(), now.toInstant()).seconds
    return when {
        secs in 0 until 60 -> "just now"
        secs in 0 until 3600 -> "${secs / 60} min ago"
        else -> dayWords(at, now, style == AgoStyle.Short)
    }
}

fun ago(at: OffsetDateTime, style: AgoStyle, now: Instant, zone: ZoneId = ZoneId.systemDefault()): String =
    ago(at, style, now.atZone(zone))
