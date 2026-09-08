package studio.kahn.iris.tv.data

import androidx.media3.common.util.UnstableApi
import androidx.media3.common.util.Util

/**
 * Cross-episode subtitle pick, shared rule with the web client
 * (`web/src/lib/iris-core/subs/pick-subtitle.ts`): keep the two in step.
 *
 * Per-file progress restores an exact track. Without one, the per-user
 * preferred LANGUAGE decides, and a language usually maps to several
 * tracks in a MULTi release: "Forcés" (forced, and often flagged default
 * by the muxer), "Complets", "SDH". Handing Media3 a preferred language
 * lets its ranking break the tie on the `default` flag, which lands on
 * the forced track: a handful of cues for foreign-language lines, which
 * reads as "subtitles off" to the viewer, on every new episode
 * (Mousetrap S01: ten times over).
 *
 * Ranking within the preferred language: non-forced before forced, and
 * among non-forced the plain track before an SDH/CC one; ties keep
 * source order. No track in the preferred language means no pick: never
 * force a different language onto the viewer.
 */
@UnstableApi
object SubtitlePick {
    private val sdhTitle = Regex("""\b(sdh|cc|hearing|malentendant|sourds?)\b""", RegexOption.IGNORE_CASE)

    /** `null` for absent / unknown (`und`) tags; `fre`, `fra` and `fr` all become `fr`. */
    fun normalizeLang(code: String?): String? {
        val base = code?.trim()?.lowercase()?.split('-', '_')?.firstOrNull().orEmpty()
        if (base.isEmpty() || base == "und") return null
        return Util.normalizeLanguageCode(base)
    }

    /** Ordinal into [subs] (the probe order, which is also Media3's text-group order), or `null`. */
    fun preferredOrdinal(subs: List<SubtitleStream>, preferredLang: String?): Int? {
        val want = normalizeLang(preferredLang) ?: return null
        val rank = { s: SubtitleStream ->
            when {
                s.forced -> 2
                sdhTitle.containsMatchIn(s.title.orEmpty()) -> 1
                else -> 0
            }
        }
        return subs.withIndex()
            .filter { normalizeLang(it.value.language) == want }
            .minByOrNull { rank(it.value) }
            ?.index
    }
}
