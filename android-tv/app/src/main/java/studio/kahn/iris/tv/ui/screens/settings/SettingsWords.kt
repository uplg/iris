package studio.kahn.iris.tv.ui.screens.settings

import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import java.util.Locale
import studio.kahn.iris.tv.data.DeviceView
import studio.kahn.iris.tv.data.PasskeyView
import studio.kahn.iris.tv.data.SubtitlePick

/**
 * A moment as people say it, past or future, as the web writes it
 * (`onDay`): "today at 21:04", "yesterday at 09:12", "on Monday",
 * "on 2 Oct", "on 2 Oct 2025".
 */
fun onDay(moment: ZonedDateTime, now: ZonedDateTime): String {
    val local = moment.withZoneSameInstant(now.zone)
    val days = ChronoUnit.DAYS.between(now.toLocalDate(), local.toLocalDate())
    val time = local.format(TIME)
    return when {
        days == 0L -> "today at $time"
        days == -1L -> "yesterday at $time"
        days == 1L -> "tomorrow at $time"
        kotlin.math.abs(days) < 7 -> "on ${local.format(WEEKDAY)}"
        local.year == now.year -> "on ${local.format(DAY_MONTH)}"
        else -> "on ${local.format(DAY_MONTH_YEAR)}"
    }
}

private val TIME = DateTimeFormatter.ofPattern("HH:mm", Locale.UK)
private val WEEKDAY = DateTimeFormatter.ofPattern("EEEE", Locale.UK)
private val DAY_MONTH = DateTimeFormatter.ofPattern("d MMM", Locale.UK)
private val DAY_MONTH_YEAR = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.UK)

private val DEVICE_KINDS = mapOf("android-tv" to "Android TV", "web" to "Web")

/** A device as the web names it: its label, else its kind, else "Unnamed device". */
fun DeviceView.name(): String =
    label?.takeIf { it.isNotBlank() } ?: kind?.let { DEVICE_KINDS[it] ?: it } ?: "Unnamed device"

/** The kind said beside a labelled device ("Android TV"), null when the name already says it. */
fun DeviceView.kindWords(): String? = if (label.isNullOrBlank()) null else kind?.let { DEVICE_KINDS[it] ?: it }

fun DeviceView.facts(now: ZonedDateTime): String =
    "Paired ${onDay(issuedAt.toZonedDateTime(), now)} · Signed in until ${onDay(expiresAt.toZonedDateTime(), now).removePrefix("on ")}"

fun PasskeyView.name(): String = name.takeIf { it.isNotBlank() } ?: "Unnamed passkey"

fun PasskeyView.facts(now: ZonedDateTime): String {
    val used = lastUsedAt?.let { "Last used ${onDay(it.toZonedDateTime(), now)}" } ?: "Never used yet"
    return "Added ${onDay(createdAt.toZonedDateTime(), now)} · $used"
}

/** The languages offered first for playback, as on the web. */
val COMMON_LANGUAGES = listOf("fr", "en", "es", "de", "it", "pt", "ja", "ko")

/** The subtitle sentinel for "no subtitles". */
const val SUBTITLES_OFF = "off"

/** A saved playback language as a choice: its ISO 639-1 base (`fre`, `fr-FR` are `fr`), or [SUBTITLES_OFF]. */
@OptIn(UnstableApi::class)
fun languageChoice(saved: String?): String? =
    if (saved == SUBTITLES_OFF) SUBTITLES_OFF else SubtitlePick.normalizeLang(saved)

/** A language code in English words ("fr" is "French"); the code in capitals when unknown. */
fun languageName(code: String): String {
    val name = Locale.forLanguageTag(code).getDisplayLanguage(Locale.ENGLISH)
    return if (name.isBlank() || name.equals(code, ignoreCase = true)) code.uppercase() else name.replaceFirstChar { it.uppercase() }
}

/** The choices of a playback language list: the common ones, plus the current one when it is not among them. */
fun languageOptions(current: String?): List<String> =
    if (current == null || current == SUBTITLES_OFF || current in COMMON_LANGUAGES) COMMON_LANGUAGES else COMMON_LANGUAGES + current

fun audioWords(choice: String?): String = choice?.let(::languageName) ?: "The file’s own"

fun subtitleWords(choice: String?): String = when (choice) {
    null -> "The file’s own"
    SUBTITLES_OFF -> "No subtitles"
    else -> languageName(choice)
}

/** The display name an email suggests: its first word ("leonard.c@…" gives "leonard"). */
fun nameFromEmail(email: String?): String =
    email?.substringBefore('@')?.substringBefore('.')?.trim().orEmpty()
