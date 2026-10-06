package studio.kahn.iris.tv.ui.screens.settings

import java.time.ZonedDateTime
import studio.kahn.iris.tv.data.DeviceView
import studio.kahn.iris.tv.data.PasskeyView
import studio.kahn.iris.tv.ui.format.languageName
import studio.kahn.iris.tv.ui.format.normalizeLanguage
import studio.kahn.iris.tv.ui.format.onDay

private val DEVICE_KINDS = mapOf("android-tv" to "Android TV", "web" to "Web")

/** A device as the web names it: its label, else its kind, else "Unnamed device". */
fun DeviceView.name(): String =
    label?.takeIf { it.isNotBlank() } ?: kind?.let { DEVICE_KINDS[it] ?: it } ?: "Unnamed device"

/** The kind said beside a labelled device ("Android TV"), null when the name already says it. */
fun DeviceView.kindWords(): String? = if (label.isNullOrBlank()) null else kind?.let { DEVICE_KINDS[it] ?: it }

fun DeviceView.facts(now: ZonedDateTime): String =
    "Paired ${onDay(issuedAt, now)} · Signed in until ${onDay(expiresAt, now).removePrefix("on ")}"

fun PasskeyView.name(): String = name.takeIf { it.isNotBlank() } ?: "Unnamed passkey"

fun PasskeyView.facts(now: ZonedDateTime): String {
    val used = lastUsedAt?.let { "Last used ${onDay(it, now)}" } ?: "Never used yet"
    return "Added ${onDay(createdAt, now)} · $used"
}

/** The languages offered first for playback, as on the web. */
val COMMON_LANGUAGES = listOf("fr", "en", "es", "de", "it", "pt", "ja", "ko")

/** The subtitle sentinel for "no subtitles". */
const val SUBTITLES_OFF = "off"

/** A saved playback language as a choice: its ISO 639-1 base (`fre`, `fr-FR` are `fr`), or [SUBTITLES_OFF]. */
fun languageChoice(saved: String?): String? =
    if (saved == SUBTITLES_OFF) SUBTITLES_OFF else normalizeLanguage(saved)

/** The choices of a playback language list: the common ones, plus the current one when it is not among them. */
fun languageOptions(current: String?): List<String> =
    if (current == null || current == SUBTITLES_OFF || current in COMMON_LANGUAGES) COMMON_LANGUAGES else COMMON_LANGUAGES + current

fun audioWords(choice: String?): String = choice?.let { languageName(it) ?: it } ?: "The file’s own"

fun subtitleWords(choice: String?): String = when (choice) {
    null -> "The file’s own"
    SUBTITLES_OFF -> "No subtitles"
    else -> languageName(choice) ?: choice
}

/** The display name an email suggests: its first word ("leonard.c@…" gives "leonard"). */
fun nameFromEmail(email: String?): String =
    email?.substringBefore('@')?.substringBefore('.')?.trim().orEmpty()
