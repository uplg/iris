package studio.kahn.iris.tv.ui.format

import java.util.Locale
import studio.kahn.iris.tv.data.MediaKind

/** An episode's code, the way the cards write it: `S2:E4`; a season alone: `Season 2`; an episode alone: `E19`. */
fun episodeCode(season: Long?, episode: Long?): String? = when {
    season == null -> episode?.let { "E$it" }
    episode == null || episode == 0L -> "Season $season"
    else -> "S$season:E$episode"
}

fun episodeCode(season: Int?, episode: Int?): String? = episodeCode(season?.toLong(), episode?.toLong())

/** `Series`, `Movie`, or null when the kind is not known. */
fun kindWord(kind: String?): String? = when (kind) {
    "tv" -> "Series"
    "movie" -> "Movie"
    else -> null
}

fun kindWord(kind: MediaKind?): String? = kindWord(kind?.value)

/** A title named in a sentence: `this film`, `this series` (a film is not a series; the web's `thisTitle`). */
fun thisTitle(kind: MediaKind?): String = if (kind == MediaKind.movie) "this film" else "this series"

/** `Movie`, `Series`, `Anime · Series` (a title with no kind reads as a movie). */
fun kindLabel(kind: MediaKind?, anime: Boolean = false): String {
    val word = kindWord(kind) ?: "Movie"
    return if (anime) "Anime · $word" else word
}

// Release tokens that end the human title in a SCENE name: a whole token must match.
private val SCENE_STOP = Regex(
    "^(19\\d{2}|20\\d{2}|s\\d{1,2}(e\\d{1,3})?|e\\d{1,3}|\\d{3,4}p|web|web-?dl|webrip|bluray|blu-?ray|brrip|bdrip|hdtv|" +
        "dvdrip|dvd|remux|x264|x265|h264|h265|hevc|avc|xvid|divx|aac\\d?|ac3|eac3|dts(-?hd)?(-?ma)?|ddp?\\d?|truehd|atmos|" +
        "flac|multi|vff|vfi|vof|vfq|vostfr|vost|vo|vf|french|truefrench|english|hdr|hdr10\\+?|dovi|dv|10bit|8bit|repack|" +
        "proper|internal|limited|uncut|unrated|extended|imax|complete|integrale)$",
    RegexOption.IGNORE_CASE,
)
private val SCENE_EXT = Regex("\\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv|srt|nfo)$", RegexOption.IGNORE_CASE)
private val SCENE_SPLIT = Regex("[._\\s]+")
private val YEAR = Regex("^(19|20)\\d{2}$")

/**
 * A raw SCENE name made readable when no verified title is known: `Mercato (2025)`. Cut at the
 * first release token, a trailing year kept; hyphens are not split ("Spider-Man").
 */
fun prettySceneName(raw: String): String {
    val tokens = raw.replace(SCENE_EXT, "").split(SCENE_SPLIT).filter { it.isNotEmpty() }
    val title = mutableListOf<String>()
    var year: String? = null
    for (t in tokens) {
        if (SCENE_STOP.matches(t)) {
            if (YEAR.matches(t) && title.isNotEmpty()) year = t
            break
        }
        title += t
    }
    if (title.isEmpty()) return tokens.joinToString(" ")
    val name = title.joinToString(" ")
    return if (year != null) "$name ($year)" else name
}

private val CODECS = listOf(
    Regex("hevc|hev1|hvc1|h265|x265", RegexOption.IGNORE_CASE) to "HEVC",
    Regex("h264|avc|x264", RegexOption.IGNORE_CASE) to "H.264",
    Regex("av1|av01", RegexOption.IGNORE_CASE) to "AV1",
    Regex("vp9|vp09", RegexOption.IGNORE_CASE) to "VP9",
    Regex("vp8", RegexOption.IGNORE_CASE) to "VP8",
    Regex("mpeg2", RegexOption.IGNORE_CASE) to "MPEG-2",
    Regex("mpeg4|xvid|divx", RegexOption.IGNORE_CASE) to "MPEG-4",
)

/** A video codec as people know it (`hevc`, `hvc1.2.4` → "HEVC", `avc1.64001f` → "H.264"); null when unknown. */
fun codecWord(codec: String?): String? =
    codec?.let { c -> CODECS.firstOrNull { (re, _) -> re.containsMatchIn(c) }?.second }

/**
 * A search language tag (`SearchResult.language_tag`: fr, en, multi, vost, vo) in words: [short]
 * fits a filter pill, [long] a release's facts; tracker jargon stays in brackets.
 */
data class LanguageTag(val tag: String, val short: String, val long: String)

val LANGUAGE_TAGS = listOf(
    LanguageTag("fr", "French (VF)", "French audio (VF)"),
    LanguageTag("en", "English", "English audio"),
    LanguageTag("multi", "Several (MULTI)", "Several audio languages (MULTI)"),
    LanguageTag("vost", "Original with subtitles (VOSTFR)", "Original audio, French subtitles (VOSTFR)"),
    LanguageTag("vo", "Original (VO)", "Original audio (VO)"),
)

fun languageLabel(tag: String?, long: Boolean = true): String? =
    LANGUAGE_TAGS.firstOrNull { it.tag == tag }?.let { if (long) it.long else it.short }

// ISO 639-2 codes (bibliographic and terminology) the tracks and the trackers carry.
private val ISO_639_2_TO_1 = mapOf(
    "fre" to "fr", "fra" to "fr", "eng" to "en", "ger" to "de", "deu" to "de",
    "spa" to "es", "ita" to "it", "por" to "pt", "dut" to "nl", "nld" to "nl",
    "jpn" to "ja", "kor" to "ko", "chi" to "zh", "zho" to "zh", "rus" to "ru",
    "ara" to "ar", "pol" to "pl", "tur" to "tr", "swe" to "sv", "nor" to "no",
    "dan" to "da", "fin" to "fi", "cze" to "cs", "ces" to "cs", "gre" to "el",
    "ell" to "el", "hun" to "hu", "rum" to "ro", "ron" to "ro", "ukr" to "uk",
    "heb" to "he", "hin" to "hi", "tha" to "th", "vie" to "vi", "ind" to "id",
    "may" to "ms", "msa" to "ms", "per" to "fa", "fas" to "fa", "cat" to "ca",
    "baq" to "eu", "eus" to "eu", "glg" to "gl", "slo" to "sk", "slk" to "sk",
    "slv" to "sl", "hrv" to "hr", "srp" to "sr", "bul" to "bg", "lit" to "lt",
    "lav" to "lv", "est" to "et", "ice" to "is", "isl" to "is", "tgl" to "tl",
    "fil" to "tl", "alb" to "sq", "sqi" to "sq", "arm" to "hy", "hye" to "hy",
    "geo" to "ka", "kat" to "ka", "mac" to "mk", "mkd" to "mk", "wel" to "cy", "cym" to "cy",
)

private val ISO3: Map<String, String> by lazy {
    Locale.getISOLanguages().associateBy { runCatching { Locale.forLanguageTag(it).isO3Language }.getOrDefault(it) }
}

/** A language tag's ISO 639-1 base (`fre`, `fra`, `fr-FR` are `fr`); null when absent or `und` (web `normalizeLang`). */
fun normalizeLanguage(code: String?): String? {
    val base = code?.trim()?.lowercase(Locale.ROOT)?.split('-', '_')?.firstOrNull().orEmpty()
    if (base.isEmpty() || base == "und") return null
    return ISO_639_2_TO_1[base] ?: if (base.length == 3) ISO3[base] ?: base else base
}

/** The saved subtitle language meaning "no subtitles" (`playback_preferences.subtitle_language`). */
const val NO_SUBTITLES = "off"

/** A saved audio language in words (Settings, a series' languages): none saved plays the file's own. */
fun audioChoiceWords(choice: String?): String = choice?.let { languageName(it) ?: it } ?: "The file’s own"

/** A saved subtitle language in words: none saved keeps the file's own, [NO_SUBTITLES] none at all. */
fun subtitleChoiceWords(choice: String?): String = when (choice) {
    null -> "The file’s own"
    NO_SUBTITLES -> "No subtitles"
    else -> languageName(choice) ?: choice
}

/** `fr`, `fre`, `fr-FR` → "French", always in English; an unknown code in capitals; absent → null. */
fun languageName(tag: String?): String? {
    val code = normalizeLanguage(tag) ?: return null
    val name = runCatching { Locale.forLanguageTag(code).getDisplayLanguage(Locale.ENGLISH) }.getOrNull()
    return if (name.isNullOrBlank() || name.equals(code, ignoreCase = true)) {
        code.uppercase(Locale.ROOT)
    } else {
        name.replaceFirstChar { it.titlecase(Locale.ENGLISH) }
    }
}
