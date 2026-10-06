package studio.kahn.iris.tv.ui.screens.player

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import studio.kahn.iris.tv.data.AudioStream
import studio.kahn.iris.tv.data.MediaProbe
import studio.kahn.iris.tv.data.SubtitleStream
import studio.kahn.iris.tv.ui.format.NO_SUBTITLES

class RouteTracksTest {
    private fun audio(index: Int, lang: String) = AudioStream(
        absoluteIndex = index, browserCompatible = true, channels = 2, codec = "aac",
        default = false, forced = false, index = index, language = lang,
    )

    private fun sub(index: Int, lang: String, text: Boolean) = SubtitleStream(
        absoluteIndex = index + 10, codec = if (text) "subrip" else "hdmv_pgs_subtitle",
        default = false, forced = false, index = index, textBased = text, language = lang,
    )

    // A PGS before the SRTs: the server's HLS only side-loads the SRTs.
    private val probe = MediaProbe(
        audio = listOf(audio(0, "fre"), audio(1, "eng")),
        container = "matroska",
        subtitle = listOf(sub(0, "fre", text = false), sub(1, "eng", text = true), sub(2, "fre", text = true)),
        video = emptyList(),
    )

    @Test
    fun theDirectPathCountsEveryStream() {
        val t = RouteTracks.of(probe, PlayRoute.Direct)
        assertEquals(2, t.subtitleOrdinal(2))
        assertEquals(2, t.subtitleIndexAt(2, groups = 3))
        assertEquals("fre", t.subtitleLanguage(t.subtitleIndexAt(0, groups = 3)))
    }

    @Test
    fun aServerRouteCountsOnlyTheTextSubtitles() {
        val t = RouteTracks.of(probe, PlayRoute.ServerRemux)
        // The second side-loaded group is the French SRT (probe index 2), not the English one.
        assertEquals(2, t.subtitleIndexAt(1, groups = 2))
        assertEquals("fre", t.subtitleLanguage(t.subtitleIndexAt(1, groups = 2)))
        assertEquals(1, t.subtitleOrdinal(2))
        // The PGS is not there to pin.
        assertNull(t.subtitleOrdinal(0))
    }

    @Test
    fun anUnexpectedGroupCountMapsNothing() {
        val t = RouteTracks.of(probe, PlayRoute.ServerTranscode)
        assertNull(t.subtitleIndexAt(0, groups = 3))
        assertNull(t.audioIndexAt(0, groups = 1))
        assertEquals(1, t.audioIndexAt(1, groups = 2))
    }

    @Test
    fun offAndNoPickStayWhatTheyAre() {
        val t = RouteTracks.of(probe, PlayRoute.Direct)
        assertEquals(-1, t.subtitleOrdinal(-1))
        assertNull(t.subtitleOrdinal(null))
        assertEquals(NO_SUBTITLES, t.subtitleLanguage(-1))
        assertEquals("eng", t.audioLanguage(1))
    }
}
