package studio.kahn.iris.tv.ui.screens.player

import java.util.Locale

/**
 * A language as people say it, from a track tag or a preference code: the
 * web's `languageName` (`fr`, `fre`, `fr-FR` → "French"; an unknown code in
 * capitals; absent or `und` → null). Always English, whatever the device
 * locale.
 */
object LanguageNames {
    private val iso6392To1 = mapOf(
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
        "fil" to "tl",
    )

    /** `null` for absent / unknown (`und`) tags; `fre`, `fra` and `fr` all become `fr`. */
    fun normalize(code: String?): String? {
        val base = code?.trim()?.lowercase(Locale.ROOT)?.split('-', '_')?.firstOrNull().orEmpty()
        if (base.isEmpty() || base == "und") return null
        return iso6392To1[base] ?: base
    }

    fun of(code: String?): String? {
        val norm = normalize(code) ?: return null
        val name = runCatching { Locale.forLanguageTag(norm).getDisplayLanguage(Locale.ENGLISH) }.getOrNull()
        return if (name.isNullOrBlank() || name.equals(norm, ignoreCase = true)) {
            norm.uppercase(Locale.ROOT)
        } else {
            name.replaceFirstChar { it.titlecase(Locale.ENGLISH) }
        }
    }
}

/** What naming an audio track needs, from the probe or, failing that, Media3's format. */
data class AudioTrackInfo(
    val language: String?,
    val title: String?,
    val channels: Int,
    val codec: String,
)

/** What naming a subtitle track needs. [textBased] false: a bitmap (PGS) or styled (ASS) track. */
data class SubtitleTrackInfo(
    val language: String?,
    val title: String?,
    val forced: Boolean,
    val textBased: Boolean,
)

/**
 * The names the audio and subtitles panel gives tracks, the web's rules
 * (`web/src/lib/player/tracks.ts`): "English, original", "French (VF)",
 * "English, audio description", "English, for deaf and hard of hearing
 * (SDH)", "English, signs and songs only"; two tracks still named alike get
 * what tells them apart (title, then channels or format, then a number).
 */
object TrackNaming {
    private val sdh = Regex("""\b(sdh|cc|hi|hearing|malentendants?|sourds?)\b""", RegexOption.IGNORE_CASE)
    private val description =
        Regex("""\b(ad|audio ?description|descriptive|described|audiodescription)\b""", RegexOption.IGNORE_CASE)
    private val signs = Regex("""\b(signs?|songs?|forced|forc[ée]s?)\b""", setOf(RegexOption.IGNORE_CASE))
    private val commentary = Regex("""\bcomment(ary|aire)\b""", RegexOption.IGNORE_CASE)
    private val frenchVariant = Regex("""\b(vff|vfq|vfi|vf2|vof|vf)\b""", RegexOption.IGNORE_CASE)
    private val original = Regex("""\b(original|vo|vost?)\b""", RegexOption.IGNORE_CASE)

    fun isSdh(title: String?): Boolean = sdh.containsMatchIn(title.orEmpty())

    private fun channelsWord(n: Int): String? = when (n) {
        1 -> "mono"
        2 -> "stereo"
        6 -> "5.1"
        8 -> "7.1"
        else -> if (n > 0) "$n channels" else null
    }

    private fun audioBase(a: AudioTrackInfo, index: Int): String {
        val lang = LanguageNames.of(a.language)
        val title = a.title.orEmpty()
        val variant = if (LanguageNames.normalize(a.language) == "fr") {
            frenchVariant.find(title)?.groupValues?.get(1)?.uppercase(Locale.ROOT)
        } else {
            null
        }
        var name = when {
            lang != null && variant != null -> "$lang ($variant)"
            lang != null -> lang
            else -> title.trim().ifEmpty { "Audio ${index + 1}" }
        }
        when {
            description.containsMatchIn(title) -> name += ", audio description"
            commentary.containsMatchIn(title) -> name += ", commentary"
            lang != null && original.containsMatchIn(title) -> name += ", original"
        }
        return name
    }

    private fun subtitleBase(s: SubtitleTrackInfo, index: Int): String {
        val lang = LanguageNames.of(s.language)
        val title = s.title.orEmpty()
        var name = lang ?: title.trim().ifEmpty { "Subtitles ${index + 1}" }
        when {
            sdh.containsMatchIn(title) -> name += ", for deaf and hard of hearing (SDH)"
            s.forced || signs.containsMatchIn(title) -> name += ", signs and songs only"
        }
        return name
    }

    private fun <T> disambiguate(items: List<T>, base: (T, Int) -> String, extras: List<(T) -> String?>): List<String> {
        val out = items.mapIndexed { i, t -> base(t, i) }.toMutableList()
        for (extra in extras) {
            val counts = out.groupingBy { it }.eachCount()
            items.forEachIndexed { i, t ->
                if ((counts[out[i]] ?: 0) < 2) return@forEachIndexed
                val e = extra(t)
                if (e != null && !out[i].lowercase(Locale.ROOT).contains(e.lowercase(Locale.ROOT))) {
                    out[i] = "${out[i]}, $e"
                }
            }
        }
        val seen = HashMap<String, Int>()
        return out.map { n ->
            val k = (seen[n] ?: 0) + 1
            seen[n] = k
            if (k > 1) "$n ($k)" else n
        }
    }

    fun audioLabels(tracks: List<AudioTrackInfo>): List<String> = disambiguate(
        tracks,
        ::audioBase,
        listOf(
            { a -> a.title?.trim()?.takeIf(String::isNotEmpty) },
            { a -> channelsWord(a.channels) },
            { a -> a.codec.uppercase(Locale.ROOT) },
        ),
    )

    fun subtitleLabels(tracks: List<SubtitleTrackInfo>): List<String> = disambiguate(
        tracks,
        ::subtitleBase,
        listOf(
            { s -> s.title?.trim()?.takeIf(String::isNotEmpty) },
            { s -> if (s.textBased) "text" else "styled" },
        ),
    )

    /** The bottom bar's summary: "English audio · English subtitles (SDH)", "French audio · No subtitles". */
    fun summary(audio: AudioTrackInfo?, subtitle: SubtitleTrackInfo?): String {
        val audioWords = audio?.let { LanguageNames.of(it.language) }?.let { "$it audio" }
        val subWords = when {
            subtitle == null -> "No subtitles"
            else -> {
                val lang = LanguageNames.of(subtitle.language) ?: "Other"
                if (isSdh(subtitle.title)) "$lang subtitles (SDH)" else "$lang subtitles"
            }
        }
        return listOfNotNull(audioWords, subWords).joinToString(" · ")
    }
}
