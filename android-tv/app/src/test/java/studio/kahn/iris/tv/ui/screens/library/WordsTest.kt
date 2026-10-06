package studio.kahn.iris.tv.ui.screens.library

import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneOffset
import org.junit.Assert.assertEquals
import org.junit.Test

class WordsTest {
    @Test
    fun countsAndShares() {
        assertEquals("1 release", plural(1, "release"))
        assertEquals("3 releases", plural(3L, "release"))
        assertEquals("2 series", plural(2, "series", "series"))
        assertEquals("42%", percent(41.6))
    }

    @Test
    fun lengthsAndClocks() {
        assertEquals("45 s", duration(45.0))
        assertEquals("55 min", duration(55 * 60.0))
        assertEquals("1 h 12 min", duration(72 * 60.0))
        assertEquals("2 h", duration(7_200.0))
        assertEquals("23 min left", timeLeft(23 * 60.0))
        assertEquals("32:10", clock(1_930.0))
        assertEquals("1:02:03", clock(3_723.0))
        assertEquals("--:--", clock(null))
    }

    @Test
    fun episodeCodes() {
        assertEquals("S2:E4", episodeCode(2, 4))
        assertEquals("Season 2", episodeCode(2, 0))
        assertEquals("E7", episodeCode(null, 7))
    }

    @Test
    fun sceneNamesMadeReadable() {
        assertEquals("Mercato (2025)", prettySceneName("Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA.mkv"))
        assertEquals("Spider-Man No Way Home (2021)", prettySceneName("Spider-Man.No.Way.Home.2021.2160p"))
        assertEquals("The.Bear.S04.1080p.WEB.H264", releaseName("The.Bear.S04.1080p.WEB.H264.mkv"))
    }

    @Test
    fun momentsAsPeopleSayThem() {
        val now = Instant.parse("2026-10-06T20:00:00Z")
        fun at(s: String) = OffsetDateTime.parse(s)
        assertEquals("today at 18:00", onDay(at("2026-10-06T18:00:00Z"), now, ZoneOffset.UTC))
        assertEquals("yesterday at 09:12", onDay(at("2026-10-05T09:12:00Z"), now, ZoneOffset.UTC))
        assertEquals("on Saturday", onDay(at("2026-10-03T10:00:00Z"), now, ZoneOffset.UTC))
        assertEquals("on 2 Sept", onDay(at("2026-09-02T10:00:00Z"), now, ZoneOffset.UTC))
        assertEquals("on 2 Oct 2025", onDay(at("2025-10-02T10:00:00Z"), now, ZoneOffset.UTC))
        assertEquals("2d ago", recentTime(at("2026-10-04T19:00:00Z"), now))
    }

    @Test
    fun languageNames() {
        assertEquals("English", languageName("en"))
        assertEquals("French", languageName("fra"))
        assertEquals("German", languageName("ger"))
        assertEquals(null, languageName(""))
    }
}
