package studio.kahn.iris.tv.ui.screens.search

import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneOffset
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.AudioInfo
import studio.kahn.iris.tv.data.MediaInfoSummary
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.SubInfo
import studio.kahn.iris.tv.data.TitleMatch
import studio.kahn.iris.tv.data.TorrentFilePreview
import studio.kahn.iris.tv.data.TorrentPreview
import studio.kahn.iris.tv.data.VideoInfo

class ReleaseWordsTest {
    @Test
    fun sceneNamesCleanedForDisplay() {
        assertEquals("Mercato (2025)", prettySceneName("Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA.mkv"))
        assertEquals("Spider-Man Far From Home (2019)", prettySceneName("Spider-Man.Far.From.Home.2019.1080p"))
        assertEquals("Severance", prettySceneName("Severance.S02.MULTi.1080p"))
        assertEquals("1080p WEB", prettySceneName("1080p.WEB"))
    }

    @Test
    fun partsAndCodes() {
        assertEquals("Season 2, complete", partWords(2, 0))
        assertEquals("S2:E7", partWords(2, 7))
        assertEquals("Complete series", partWords(null, null, "Show.INTEGRALE.1080p"))
        assertNull(partWords(null, null, "Movie.2006.1080p"))
        assertEquals("Season 2", episodeCode(2, null))
        assertEquals("E5", episodeCode(null, 5))
    }

    @Test
    fun picturesAndSwarms() {
        assertEquals("2160p", resolution("Show.S02.2160p.DV"))
        assertEquals("2160p", resolution("Show.4K.HDR"))
        assertEquals("720p", resolution("Show.720p.WEB"))
        assertNull(resolution("Show.WEB"))
        assertEquals("HEVC", codecWord("hevc"))
        assertNull(codecWord("unknown"))
        assertEquals("1 seeder", seedersWords(1))
        assertEquals("1,204 seeders", seedersWords(1204))
        assertTrue(isDead(0))
        assertFalse(isDead(null))
    }

    @Test
    fun gridAndListLines() {
        val tv = TitleMatch(kind = MediaKind.tv, title = "Severance", tmdbId = 1, year = 2022)
        val r = SearchResult(
            externalId = "2",
            providerId = "v3x",
            title = "Severance.S02.VOSTFR.1080p.WEB.H264",
            languageTag = "vost",
            parsedSeason = 2,
            parsedEpisode = 0,
            seeders = 61,
            sizeBytes = 15_891_378_995,
            titleMatch = tv,
            uploadedAt = OffsetDateTime.of(2026, 9, 29, 12, 0, 0, 0, ZoneOffset.UTC),
        )
        assertEquals("v3x:2", releaseKey(r))
        assertEquals("Severance", titleOf(r))
        assertEquals("Season 2 · VOSTFR", gridWhat(r))
        assertEquals("61 seeders · 14.8 GB · v3x · 7d ago", factsLine(r, Instant.parse("2026-10-06T12:00:00Z")))
        val movie = r.copy(titleMatch = tv.copy(kind = MediaKind.movie, year = 2006), parsedSeason = null, parsedEpisode = null, languageTag = "multi")
        assertEquals("Movie 2006 · MULTI", gridWhat(movie))
        assertNull(ownedFile(r))
        assertEquals(OwnedFile("abc", 3), ownedFile(r.copy(alreadyInLibrary = true, libraryInfohash = "abc", libraryFileIdx = 3)))
    }

    @Test
    fun relativeTimesAsTheWebSaysThem() {
        val now = Instant.parse("2026-10-06T12:00:00Z")
        fun ago(days: Long) = OffsetDateTime.ofInstant(now.minusSeconds(days * 86_400), ZoneOffset.UTC)
        assertEquals("today", formatRelative(ago(0), now))
        assertEquals("3d ago", formatRelative(ago(3), now))
        assertEquals("2mo ago", formatRelative(ago(65), now))
        assertEquals("1y ago", formatRelative(ago(400), now))
    }

    @Test
    fun technicalSheetInWords() {
        val mi = MediaInfoSummary(
            video = VideoInfo(codec = "AVC", resolution = "1920x1080", fps = 25f, durationSecs = 4320),
            audio = listOf(AudioInfo(lang = "English", commercialName = "Dolby Atmos", channels = 8), AudioInfo(codec = "AAC", channels = 2)),
            subtitles = listOf(SubInfo(lang = "French", forced = true, format = "PGS"), SubInfo(lang = "English", title = "SDH")),
        )
        assertEquals("AVC · 1920x1080 · 25 fps · 1 h 12 min", videoWords(mi))
        assertEquals("English (Dolby Atmos, 7.1), Unknown language (AAC, stereo)", audioWords(mi))
        assertEquals("French (PGS, forced), English (for the deaf and hard of hearing)", subtitleWords(mi))
        assertNull(videoWords(null))
        assertEquals("45 s", durationWords(45))
        assertEquals("2 h", durationWords(7200))
    }

    @Test
    fun releaseSheetSaysWhyAGrabCannotStart() {
        val preview = TorrentPreview(
            announceUrls = emptyList(),
            files = listOf(
                TorrentFilePreview(index = 0, isArchive = false, isVideo = true, path = "S/Show.S01E02.mkv", sizeBytes = 10),
                TorrentFilePreview(index = 1, isArchive = false, isVideo = true, path = "S/Show.S01E01.mkv", sizeBytes = 10),
            ),
            infohash = "x",
            name = "Show.S01.1080p",
            pieceCount = 1,
            pieceLength = 1,
            streamable = true,
            totalSizeBytes = 20,
        )
        val state = ReleaseUiState(providerId = "torr9", externalId = "1", preview = studio.kahn.iris.tv.ui.state.Loadable.Ready(preview))
        val sheet = state.sheet
        assertEquals("Show · Season 1, complete", sheet.heading)
        assertTrue(sheet.isTv)
        assertEquals(1, sheet.chosenFile)
        assertEquals("Download and play episode 1", sheet.playLabel)
        assertNull(sheet.blocked)
        assertEquals(ARCHIVE_WORDS, state.copy(preview = studio.kahn.iris.tv.ui.state.Loadable.Ready(preview.copy(streamable = false))).sheet.blocked)
        val dead = state.copy(hit = SearchResult(externalId = "1", providerId = "torr9", title = "Show.S01.1080p", seeders = 0)).sheet
        assertEquals("$DEAD: this release cannot be downloaded. Try another one.", dead.blocked)
        assertEquals("Swarm" to "0 seeders", dead.facts.first())
    }
}
