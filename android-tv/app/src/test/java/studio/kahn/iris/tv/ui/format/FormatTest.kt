package studio.kahn.iris.tv.ui.format

import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import studio.kahn.iris.tv.data.MediaKind

/** The shared words say what the web app's say (`@iris/api/format`, `history/words.ts`, `language.ts`). */
class FormatTest {
    @Test
    fun sizesAndSpeeds() {
        assertEquals("512 B", formatSize(512))
        assertEquals("1.5 KB", formatSize(1536))
        assertEquals("12.4 GB", formatSize((12.4 * 1024 * 1024 * 1024).toLong()))
        assertEquals("120 MB", formatSize(120L * 1024 * 1024))
        assertEquals("?", formatSize(-1))
        assertEquals("6.1 MB/s", formatSpeed((6.1 * 1024 * 1024).toLong()))
    }

    @Test
    fun countsAndShares() {
        assertEquals("1 release", plural(1, "release"))
        assertEquals("3 releases", plural(3L, "release"))
        assertEquals("2 series", plural(2, "series", "series"))
        assertEquals("42%", percent(41.6))
        assertEquals("64%", percent(63.6))
    }

    @Test
    fun clocksAndLengths() {
        assertEquals("32:10", clock(1_930.0))
        assertEquals("1:02:03", clock(3_723.0))
        assertEquals("--:--", clock(null))
        assertEquals("--:--", clock(-1.0))
        assertEquals("45 s", duration(45.0))
        assertEquals("55 min", duration(55 * 60.0))
        assertEquals("1 h 12 min", duration(72 * 60.0))
        assertEquals("2 h", duration(7_200.0))
        assertEquals("1 h", duration(3_599.0))
        assertEquals("2 h", duration(7_170.0))
        assertEquals("23 min left", timeLeft(23 * 60.0))
    }

    @Test
    fun aPastMomentIsSaidOneWay() {
        val now = ZonedDateTime.of(2026, 10, 6, 21, 0, 0, 0, ZoneOffset.UTC)
        fun back(seconds: Long) = OffsetDateTime.ofInstant(now.toInstant().minusSeconds(seconds), ZoneOffset.UTC)
        assertEquals("just now", ago(back(5), now = now))
        assertEquals("12 min ago", ago(back(12 * 60 + 30), now = now))
        assertEquals("today at 19:00", ago(back(2 * 3_600), now = now))
        assertEquals("today 19:00", ago(back(2 * 3_600), AgoStyle.Short, now))
        assertEquals("yesterday at 21:04", ago(OffsetDateTime.of(2026, 10, 5, 21, 4, 0, 0, ZoneOffset.UTC), now = now))
        assertEquals("yesterday 21:04", ago(OffsetDateTime.of(2026, 10, 5, 21, 4, 0, 0, ZoneOffset.UTC), AgoStyle.Short, now))
        assertEquals("on Saturday", ago(back(3 * 86_400), now = now))
        assertEquals("Saturday", ago(back(3 * 86_400), AgoStyle.Short, now))
        assertEquals("on 3 Oct 2025", ago(OffsetDateTime.of(2025, 10, 3, 12, 0, 0, 0, ZoneOffset.UTC), now = now))
        assertEquals("3 Oct 2025", ago(OffsetDateTime.of(2025, 10, 3, 12, 0, 0, 0, ZoneOffset.UTC), AgoStyle.Short, now))
        assertEquals("tomorrow at 08:00", onDay(OffsetDateTime.of(2026, 10, 7, 8, 0, 0, 0, ZoneOffset.UTC), now))
    }

    @Test
    fun momentsAsPeopleSayThem() {
        val paris = ZoneId.of("Europe/Paris")
        val now = ZonedDateTime.of(2026, 10, 6, 14, 0, 0, 0, paris)
        fun at(z: ZonedDateTime) = z.toOffsetDateTime()
        assertEquals("today at 09:05", onDay(at(now.withHour(9).withMinute(5)), now))
        assertEquals("yesterday at 21:04", onDay(at(now.minusDays(1).withHour(21).withMinute(4)), now))
        assertEquals("tomorrow at 08:00", onDay(at(now.plusDays(1).withHour(8)), now))
        assertEquals("on Friday", onDay(at(now.minusDays(4)), now))
        assertEquals("on 2 Sept", onDay(at(now.withMonth(9).withDayOfMonth(2)), now))
        assertEquals("on 2 Oct 2025", onDay(at(now.minusYears(1).withDayOfMonth(2)), now))
        // A moment elsewhere is said in the TV's zone.
        assertEquals("today at 01:30", onDay(OffsetDateTime.parse("2026-10-05T23:30:00Z"), now))
    }

    @Test
    fun episodeCodes() {
        assertEquals("S2:E4", episodeCode(2, 4))
        assertEquals("S2:E4", episodeCode(2L, 4L))
        assertEquals("Season 2", episodeCode(2, null))
        assertEquals("Season 2", episodeCode(2, 0))
        assertEquals("E19", episodeCode(null, 19))
        assertNull(episodeCode(null as Int?, null))
    }

    @Test
    fun kinds() {
        assertEquals("Series", kindWord("tv"))
        assertEquals("Movie", kindWord(MediaKind.movie))
        assertNull(kindWord("person"))
        assertEquals("Series", kindLabel(MediaKind.tv))
        assertEquals("Movie", kindLabel(null))
        assertEquals("Anime · Series", kindLabel(MediaKind.tv, anime = true))
    }

    @Test
    fun sceneNamesReadAsTitles() {
        assertEquals("Mercato (2025)", prettySceneName("Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA.mkv"))
        assertEquals("Spider-Man No Way Home (2021)", prettySceneName("Spider-Man.No.Way.Home.2021.2160p.mkv"))
        assertEquals("Severance", prettySceneName("Severance.S02E04.1080p.WEB.mkv"))
        assertEquals("Severance", prettySceneName("Severance.S02.MULTi.1080p"))
        assertEquals("1080p WEB", prettySceneName("1080p.WEB.mkv"))
    }

    @Test
    fun languagesInEnglishWhateverTheTag() {
        assertEquals("French", languageName("fre"))
        assertEquals("French", languageName("fra"))
        assertEquals("French", languageName("fr-FR"))
        assertEquals("English", languageName("en"))
        assertEquals("German", languageName("ger"))
        assertEquals("Japanese", languageName("ja"))
        assertEquals("XX", languageName("xx"))
        assertNull(languageName("und"))
        assertNull(languageName(""))
        assertNull(languageName(null))
        assertEquals("fr", normalizeLanguage("FRE"))
        assertEquals("fr", normalizeLanguage("fr-FR"))
    }
}
