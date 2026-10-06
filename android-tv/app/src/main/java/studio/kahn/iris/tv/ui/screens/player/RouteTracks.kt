package studio.kahn.iris.tv.ui.screens.player

import studio.kahn.iris.tv.data.AudioStream
import studio.kahn.iris.tv.data.MediaProbe
import studio.kahn.iris.tv.data.SubtitleStream
import studio.kahn.iris.tv.ui.format.OFF

/**
 * The probed streams in the order the player's track groups come on [route]. The direct
 * path surfaces every stream of the file in source order; the server's HLS carries the audio
 * and only the TEXT-based subtitles, side-loaded as WebVTT (a PGS cannot become one). A pick
 * is an ordinal into these lists, never into the probe's own (`trackMenu` reads them the same).
 */
data class RouteTracks(val audio: List<AudioStream>, val subtitles: List<SubtitleStream>) {
    /** The ordinal of the audio stream [index] (a probe index), or null. */
    fun audioOrdinal(index: Int?): Int? = index?.let { i -> audio.indexOfFirst { it.index == i }.takeIf { it >= 0 } }

    /** The ordinal of the subtitle stream [index]; -1 stays "turned off", null "no pick". */
    fun subtitleOrdinal(index: Int?): Int? = when (index) {
        null -> null
        -1 -> -1
        else -> subtitles.indexOfFirst { it.index == index }.takeIf { it >= 0 }
    }

    /**
     * The probe index of the [ordinal]-th audio group among [groups] surfaced, or null when the
     * player surfaced another count than probed (the mapping is then unknown).
     */
    fun audioIndexAt(ordinal: Int, groups: Int): Int? = audio.takeIf { it.size == groups }?.getOrNull(ordinal)?.index

    /** As [audioIndexAt], for the subtitles. */
    fun subtitleIndexAt(ordinal: Int, groups: Int): Int? =
        subtitles.takeIf { it.size == groups }?.getOrNull(ordinal)?.index

    /** The language of the audio stream [index]. */
    fun audioLanguage(index: Int?): String? = index?.let { i -> audio.firstOrNull { it.index == i }?.language }

    /** The language kept for the series: "off" when turned off. */
    fun subtitleLanguage(index: Int?): String? = when (index) {
        null -> null
        -1 -> OFF
        else -> subtitles.firstOrNull { it.index == index }?.language
    }

    companion object {
        fun of(probe: MediaProbe, route: PlayRoute) = RouteTracks(
            audio = probe.audio,
            subtitles = if (route == PlayRoute.Direct) probe.subtitle else probe.subtitle.filter { it.textBased },
        )
    }
}
