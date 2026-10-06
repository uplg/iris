package studio.kahn.iris.tv.ui.screens.player

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The web's rules (`web/src/lib/player/tracks.test.ts`), on the TV. */
class TrackNamingTest {
    private val english = AudioTrackInfo("eng", "Original", 6, "eac3")
    private val frenchVf = AudioTrackInfo("fre", "VFF", 6, "ac3")

    @Test
    fun languagesInEnglishWhateverTheTag() {
        assertEquals("French", LanguageNames.of("fre"))
        assertEquals("French", LanguageNames.of("fra"))
        assertEquals("French", LanguageNames.of("fr-FR"))
        assertEquals("German", LanguageNames.of("ger"))
        assertNull(LanguageNames.of("und"))
        assertNull(LanguageNames.of(null))
        assertEquals("fr", LanguageNames.normalize("FRE"))
    }

    @Test
    fun namesAudioUnambiguously() {
        assertEquals(listOf("English, original", "French (VFF)"), TrackNaming.audioLabels(listOf(english, frenchVf)))
        val ad = english.copy(title = "Audio Description")
        assertEquals(listOf("English, original", "English, audio description"), TrackNaming.audioLabels(listOf(english, ad)))
        assertEquals(listOf("English, commentary"), TrackNaming.audioLabels(listOf(english.copy(title = "Director's commentary"))))
    }

    @Test
    fun tellsTwinsApart() {
        val twins = listOf(english.copy(title = null, channels = 6), english.copy(title = null, channels = 2))
        assertEquals(listOf("English, 5.1", "English, stereo"), TrackNaming.audioLabels(twins))
        val same = listOf(english.copy(title = null), english.copy(title = null))
        assertEquals(listOf("English, 5.1, EAC3", "English, 5.1, EAC3 (2)"), TrackNaming.audioLabels(same))
    }

    @Test
    fun untaggedAudioFallsBackToItsTitleThenANumber() {
        assertEquals(
            listOf("Japanese dub", "Audio 2"),
            TrackNaming.audioLabels(listOf(AudioTrackInfo(null, "Japanese dub", 2, "aac"), AudioTrackInfo("und", null, 2, "aac"))),
        )
    }

    @Test
    fun namesSubtitlesBySdhAndSigns() {
        val labels = TrackNaming.subtitleLabels(
            listOf(
                SubtitleTrackInfo("eng", "English SDH", forced = false, textBased = true),
                SubtitleTrackInfo("eng", "Signs & Songs", forced = false, textBased = true),
                SubtitleTrackInfo("fre", "Forcés", forced = true, textBased = true),
                SubtitleTrackInfo("fre", "Complets", forced = false, textBased = true),
            ),
        )
        assertEquals(
            listOf(
                "English, for deaf and hard of hearing (SDH)",
                "English, signs and songs only",
                "French, signs and songs only",
                "French",
            ),
            labels,
        )
    }

    @Test
    fun twoPlainSubtitlesInOneLanguageShowTheirTitles() {
        val labels = TrackNaming.subtitleLabels(
            listOf(
                SubtitleTrackInfo("eng", "Full", forced = false, textBased = true),
                SubtitleTrackInfo("eng", "Full", forced = false, textBased = false),
            ),
        )
        assertEquals(listOf("English, Full, text", "English, Full, styled"), labels)
    }

    @Test
    fun theSummaryLine() {
        assertEquals(
            "English audio · English subtitles (SDH)",
            TrackNaming.summary(english, SubtitleTrackInfo("eng", "SDH", forced = false, textBased = true)),
        )
        assertEquals("French audio · No subtitles", TrackNaming.summary(frenchVf, null))
    }
}
