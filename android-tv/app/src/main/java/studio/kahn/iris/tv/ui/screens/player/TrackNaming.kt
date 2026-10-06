package studio.kahn.iris.tv.ui.screens.player

import studio.kahn.iris.tv.ui.format.NO_SUBTITLES
import studio.kahn.iris.tv.ui.format.SUBTITLES_OFF
import java.util.Locale
import studio.kahn.iris.tv.ui.format.languageName
import studio.kahn.iris.tv.ui.format.normalizeLanguage

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
        val lang = languageName(a.language)
        val title = a.title.orEmpty()
        val variant = if (normalizeLanguage(a.language) == "fr") {
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
        val lang = languageName(s.language)
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

    /** The bottom bar's summary: "English audio · English subtitles (SDH)", "French audio · Subtitles off". */
    fun summary(audio: AudioTrackInfo?, subtitle: SubtitleTrackInfo?, hasSubtitles: Boolean = subtitle != null): String {
        val audioWords = audio?.let { languageName(it.language) }?.let { "$it audio" }
        val subWords = when {
            subtitle == null -> if (hasSubtitles) SUBTITLES_OFF else NO_SUBTITLES
            else -> {
                val lang = languageName(subtitle.language) ?: "Other"
                if (isSdh(subtitle.title)) "$lang subtitles (SDH)" else "$lang subtitles"
            }
        }
        return listOfNotNull(audioWords, subWords).joinToString(" · ")
    }
}
