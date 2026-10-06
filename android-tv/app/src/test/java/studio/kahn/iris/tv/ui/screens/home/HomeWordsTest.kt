package studio.kahn.iris.tv.ui.screens.home

import java.time.OffsetDateTime
import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.DiskSpace
import studio.kahn.iris.tv.data.HomeSummary
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView

/** The home's words say what the web app's say (`web/src/lib/home/data.test.ts`, `@iris/api/format`). */
// Robolectric: language names go through Media3's code normalisation, which reads android.*.
@RunWith(RobolectricTestRunner::class)
class HomeWordsTest {
    @Test
    fun clocksAndLengths() {
        assertEquals("32:10", clock(1930.0))
        assertEquals("1:02:03", clock(3723.0))
        assertEquals("45 s", duration(45.0))
        assertEquals("55 min", duration(3300.0))
        assertEquals("1 h 12 min", duration(4320.0))
        assertEquals("2 h", duration(7200.0))
        assertEquals("23 min left", timeLeft(1380.0))
    }

    @Test
    fun episodeCodes() {
        assertEquals("S2:E4", episodeCode(2, 4))
        assertEquals("Season 2", episodeCode(2, null))
        assertEquals("Season 2", episodeCode(2, 0))
        assertEquals("E19", episodeCode(null, 19))
        assertNull(episodeCode(null, null))
    }

    @Test
    fun kindsAndCounts() {
        assertEquals("Series", kindLabel(MediaKind.tv))
        assertEquals("Movie", kindLabel(null))
        assertEquals("Anime · Series", kindLabel(MediaKind.tv, anime = true))
        assertEquals("1 download", plural(1, "download"))
        assertEquals("3 series", plural(3, "series", "series"))
        assertEquals("64%", percent(63.6))
    }

    @Test
    fun sceneNamesReadAsTitles() {
        assertEquals("Mercato (2025)", prettySceneName("Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA.mkv"))
        assertEquals("Spider-Man No Way Home (2021)", prettySceneName("Spider-Man.No.Way.Home.2021.2160p.mkv"))
        assertEquals("Severance", prettySceneName("Severance.S02E04.1080p.WEB.mkv"))
        assertEquals("1080p WEB", prettySceneName("1080p.WEB.mkv"))
    }

    @Test
    fun rightNowSaysOnlyWhatHappens() {
        val gb = 1024L * 1024 * 1024
        assertEquals(
            listOf("2 downloads · 64% · 23 min left", "3 new episodes on your watchlist", "412 GB free on disk", "Seeding 18 releases"),
            rightNow(HomeSummary(2, 63.6, 3, 18, DiskSpace(412 * gb, 2000 * gb), 1380)),
        )
        assertEquals(
            listOf("1 download · 10%", "1 new episode on your watchlist", "Seeding 1 release"),
            rightNow(HomeSummary(1, 10.0, 1, 1)),
        )
        assertEquals(emptyList<String>(), rightNow(HomeSummary(0, 0.0, 0, 0)))
    }

    @Test
    fun downloadsPerCollectionByBytes() {
        val a = UUID.randomUUID()
        val b = UUID.randomUUID()
        val list = listOf(torrent(a, 1, 4), torrent(a, 3, 4), torrent(b, 5, 5, finished = true), torrent(null, 1, 2))
        assertEquals(mapOf(a.toString() to 50.0), downloadsByCollection(list))
    }

    @Test
    fun timeLeftOnlyWhenTheLengthIsKnown() {
        assertEquals(1370.0, secondsLeft(cw(position = 1930.0, duration = 3300.0))!!, 0.0)
        assertEquals(0.5f, watchedShare(cw(position = 1650.0, duration = 3300.0))!!, 0f)
        assertNull(secondsLeft(cw(position = 10.0, duration = null)))
    }

    @Test
    fun languagesAPlayWillUse() {
        assertEquals("Plays with audio in French, subtitles off.", languagesLine(PlaybackPrefsResponse(audioLanguage = "fr", subtitleLanguage = "off")))
        assertEquals(
            "Plays with subtitles in English, as chosen for this series.",
            languagesLine(PlaybackPrefsResponse(subtitleLanguage = "eng", forCollection = true)),
        )
        assertNull(languagesLine(PlaybackPrefsResponse()))
        assertNull(languagesLine(null))
    }

    companion object {
        fun cw(
            position: Double = 0.0,
            duration: Double? = null,
            season: Long? = null,
            episode: Long? = null,
            collection: UUID? = null,
            grabbable: Boolean = false,
            nextUp: Boolean = false,
            kind: MediaKind? = null,
            name: String = "Severance.S02E04.1080p.WEB.mkv",
        ) = ContinueWatchingItem(
            completed = false,
            fileIdx = 0,
            grabbable = grabbable,
            infohash = if (grabbable) "" else "abc",
            lastWatchedAt = OffsetDateTime.parse("2026-10-01T20:00:00Z"),
            nextUp = nextUp,
            positionSeconds = position,
            tmdbVerified = false,
            torrentName = name,
            collectionId = collection,
            durationSeconds = duration,
            episode = episode,
            kind = kind,
            season = season,
        )

        fun torrent(collection: UUID?, done: Long, total: Long, finished: Boolean = false) = TorrentView(
            downloadSpeedBps = 0,
            fetchedAt = OffsetDateTime.parse("2026-10-01T20:00:00Z"),
            files = emptyList(),
            finished = finished,
            infohash = UUID.randomUUID().toString(),
            peers = 0,
            progressBytes = done,
            progressPct = done * 100.0 / total,
            state = TorrentState.live,
            totalSizeBytes = total,
            uploadSpeedBps = 0,
            uploadedBytes = 0,
            addedAt = OffsetDateTime.parse("2026-10-01T20:00:00Z"),
            addedBy = UUID.randomUUID(),
            addedByName = "Leonard",
            id = UUID.randomUUID(),
            tmdbVerified = false,
            uploadedBytesTotal = 0,
            collectionId = collection,
        )
    }
}
