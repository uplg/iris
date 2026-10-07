package studio.kahn.iris.tv.ui.screens.home

import java.time.OffsetDateTime
import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.DiskSpace
import studio.kahn.iris.tv.data.HomeSummary
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView

/** The home's words say what the web app's say (`web/src/lib/home/data.test.ts`, `@iris/api/format`). */
class HomeWordsTest {
    @Test
    fun rightNowSaysOnlyWhatDownloads() {
        val gb = 1024L * 1024 * 1024
        assertEquals(
            listOf("2 downloads · 64% · 23 min left"),
            rightNow(HomeSummary(2, 63.6, 3, 18, DiskSpace(412 * gb, 2000 * gb), 1380)),
        )
        assertEquals(listOf("1 download · 10%"), rightNow(HomeSummary(1, 10.0, 1, 1)))
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
