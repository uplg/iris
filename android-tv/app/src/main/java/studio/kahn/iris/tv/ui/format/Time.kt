package studio.kahn.iris.tv.ui.format

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
    val h = total / 3600
    val m = ((total % 3600) / 60.0).roundToLong()
    if (h == 0L) return "$m min"
    return if (m == 0L) "$h h" else "$h h $m min"
}

/** What is left to watch: `23 min left`. */
fun timeLeft(seconds: Double): String = "${duration(seconds)} left"

/** Recent activity, finer than a day: `just now`, `5m ago`, `3h ago`, `2d ago` (web `formatRecentTime`). */
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

/** An upload's age in whole days: `today`, `3d ago`, `2mo ago`, `1y ago` (web `formatRelative`). */
fun formatRelative(at: OffsetDateTime, now: Instant = Instant.now()): String {
    val days = ChronoUnit.DAYS.between(at.toInstant(), now)
    if (days < 1) return "today"
    if (days < 30) return "${days}d ago"
    val months = days / 30
    if (months < 12) return "${months}mo ago"
    return "${months / 12}y ago"
}

private val TIME = DateTimeFormatter.ofPattern("HH:mm", Locale.UK)
private val DAY_MONTH = DateTimeFormatter.ofPattern("d MMM", Locale.UK)
private val DAY_MONTH_YEAR = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.UK)

/**
 * A moment as people say it, past or future, in [now]'s zone: `today at 21:04`,
 * `yesterday at 09:12`, `tomorrow at 08:00`, `on Monday`, `on 2 Oct`, `on 2 Oct 2025`.
 */
fun onDay(at: OffsetDateTime, now: ZonedDateTime = ZonedDateTime.now()): String {
    val local = at.atZoneSameInstant(now.zone)
    val days = ChronoUnit.DAYS.between(now.toLocalDate(), local.toLocalDate())
    return when {
        days == 0L -> "today at ${local.format(TIME)}"
        days == -1L -> "yesterday at ${local.format(TIME)}"
        days == 1L -> "tomorrow at ${local.format(TIME)}"
        abs(days) < 7 -> "on ${local.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.UK)}"
        local.year == now.year -> "on ${local.format(DAY_MONTH)}"
        else -> "on ${local.format(DAY_MONTH_YEAR)}"
    }
}
