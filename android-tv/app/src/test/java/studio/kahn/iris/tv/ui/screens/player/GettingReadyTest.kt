package studio.kahn.iris.tv.ui.screens.player

import java.time.OffsetDateTime
import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.AvailableEpisodeEntry
import studio.kahn.iris.tv.data.EpisodeEntry
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.data.PlayStatus
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.ui.components.StepState

internal fun torrent(
    state: TorrentState = TorrentState.live,
    pct: Double = 64.0,
    peers: Int = 38,
    speed: Long = 8L * 1024 * 1024,
    finished: Boolean = false,
    ageMinutes: Long = 1,
    files: List<FileEntry> = listOf(FileEntry(0, "Show.S02E01.mkv", 1_000)),
    error: String? = null,
): TorrentView {
    val added = OffsetDateTime.parse("2026-10-06T20:00:00Z")
    return TorrentView(
        downloadSpeedBps = speed,
        fetchedAt = added.plusMinutes(ageMinutes),
        files = files,
        finished = finished,
        infohash = "ab",
        peers = peers,
        progressBytes = 0,
        progressPct = pct,
        state = state,
        totalSizeBytes = 1_000,
        uploadSpeedBps = 0,
        uploadedBytes = 0,
        addedAt = added,
        addedBy = UUID.randomUUID(),
        addedByName = "Léo",
        id = UUID.randomUUID(),
        tmdbVerified = true,
        uploadedBytesTotal = 0,
        error = error,
    )
}

class GettingReadyTest {
    @Test
    fun theBoardsMoment() {
        val r = readiness(ReadyInput(torrent(), ProbePhase.Waiting(notOnDisk = true)))
        assertNull(r.problem)
        assertEquals(
            listOf("Connected to peers", "Downloading the first minutes", "Read the video details", "Start playback"),
            r.steps.map { it.label },
        )
        assertEquals(listOf(StepState.Done, StepState.Current, StepState.Pending, StepState.Pending), r.steps.map { it.state })
        assertEquals("38 peers", r.steps[0].detail)
        assertEquals("64 % · 8.0 MB/s", r.steps[1].detail)
        assertEquals(0.64f, r.steps[1].progress!!, 0.001f)
    }

    @Test
    fun stillConnecting() {
        val r = readiness(ReadyInput(torrent(state = TorrentState.initializing, peers = 0, speed = 0, pct = 0.0), ProbePhase.Waiting()))
        assertEquals("Connecting to peers", r.steps[0].label)
        assertEquals(StepState.Current, r.steps[0].state)
        assertEquals("Download the first minutes", r.steps[1].label)
    }

    @Test
    fun readThenStarting() {
        val r = readiness(ReadyInput(torrent(), ProbePhase.Done, playWords = "1080p H.264, plays directly"))
        assertEquals(StepState.Done, r.steps[2].state)
        assertEquals("1080p H.264, plays directly", r.steps[2].detail)
        assertEquals("Starting playback", r.steps.last().label)
        assertEquals(StepState.Current, r.steps.last().state)
    }

    @Test
    fun theServerStepWhenTheServerBuildsTheStream() {
        val r = readiness(
            ReadyInput(
                torrent(finished = true, pct = 100.0),
                ProbePhase.Done,
                serverPrep = true,
                playStatus = PlayStatus(ready = false, progress = 0.42, reason = "remuxing"),
            ),
        )
        val server = r.steps[3]
        assertEquals("Preparing the stream on the server", server.label)
        assertEquals(StepState.Current, server.state)
        assertEquals("42 %", server.detail)
        assertEquals(StepState.Pending, r.steps.last().state)
    }

    @Test
    fun aDeadSwarmIsSaid() {
        val dead = torrent(peers = 0, speed = 0, pct = 12.0, ageMinutes = 3)
        assertTrue(isDeadSwarm(dead, null))
        assertFalse(isDeadSwarm(dead.copy(fetchedAt = dead.addedAt.plusSeconds(30)), null))
        assertTrue(isDeadSwarm(torrent(), "no seeders answered"))
        val r = readiness(ReadyInput(dead, ProbePhase.Waiting()))
        assertTrue(r.problem!!.deadSwarm)
        assertEquals("Nobody is sharing this release", r.problem.title)
    }

    @Test
    fun otherProblems() {
        assertEquals(
            "The torrent stopped with an error",
            readiness(ReadyInput(torrent(state = TorrentState.error, error = "disk full"), ProbePhase.Waiting())).problem?.title,
        )
        assertEquals(
            "Iris could not read this file",
            readiness(ReadyInput(torrent(), ProbePhase.Failed("bad header"))).problem?.title,
        )
        assertEquals(
            "The server could not prepare this file",
            readiness(
                ReadyInput(torrent(), ProbePhase.Done, serverPrep = true, playStatus = PlayStatus(ready = false, error = "ffmpeg died")),
            ).problem?.title,
        )
    }

    @Test
    fun pictureAndFacts() {
        assertEquals("2160p HEVC Dolby Vision", pictureWords(2160, "hevc", "dovi"))
        assertEquals("1080p H.264", pictureWords(1080, "h264", "sdr"))
        assertEquals("1080p AV1, converted on the server", playWords("1080p AV1", PlayRoute.ServerTranscode))
        assertEquals("Plays directly", playWords(null, PlayRoute.Direct))
        assertEquals("Playing from disk · 1080p HEVC", factsLine(torrent(finished = true), "1080p HEVC"))
        assertEquals("Playing while it downloads, 64 %", factsLine(torrent(), null))
    }
}

class SideRowsTest {
    private fun ep(season: Long, episode: Long, infohash: String, idx: Long, language: String? = "english", watched: Boolean = false) =
        EpisodeEntry(episode = episode, fileIdx = idx, infohash = infohash, season = season, watched = watched, language = language)

    private fun available(season: Long, episode: Long, language: String?) = AvailableEpisodeEntry(
        episode = episode,
        foundAt = OffsetDateTime.parse("2026-10-06T20:00:00Z"),
        indexerProvider = "p",
        indexerTorrentId = "$season$episode$language",
        season = season,
        language = language,
    )

    @Test
    fun theSeasonOneRowPerEpisode() {
        val rows = sideRows(
            SideInput(
                infohash = "a",
                fileIdx = 1,
                isTvCollection = true,
                episodes = listOf(
                    ep(2, 1, "a", 0, watched = true),
                    ep(2, 2, "a", 1),
                    ep(2, 2, "b", 0, language = "french"),
                    ep(1, 9, "c", 0),
                ),
                available = listOf(available(2, 3, "french"), available(2, 3, "english"), available(2, 2, "french")),
                videoFiles = emptyList(),
                progressByFile = mapOf(1 to FileProgressEntry(completed = false, fileIdx = 1, lastWatchedAt = OffsetDateTime.now(), positionSeconds = 600.0, durationSeconds = 1_200.0)),
                names = mapOf((2L to 2L) to "Goodbye"),
            ),
        )
        assertEquals(listOf("S2:E1", "S2:E2 · Goodbye", "S2:E3"), rows.map { it.primary })
        assertTrue(rows[0].watched)
        assertTrue(rows[1].active)
        assertEquals(50.0, rows[1].watchedPct!!, 0.01)
        assertTrue(rows[1].started)
        assertEquals(GrabTarget(2, 3, "english"), rows[2].grab)
    }

    @Test
    fun otherwiseTheTorrentsFiles() {
        val rows = sideRows(
            SideInput(
                infohash = "a",
                fileIdx = 0,
                isTvCollection = false,
                episodes = emptyList(),
                available = emptyList(),
                videoFiles = listOf(FileEntry(0, "Movie/Movie.mkv", 2048), FileEntry(2, "Movie/Extras.mkv", 1024)),
                progressByFile = emptyMap(),
            ),
        )
        assertEquals(listOf("Movie.mkv", "Extras.mkv"), rows.map { it.primary })
        assertEquals("2.0 KB", rows[0].secondary)
        assertTrue(rows[0].active)
        assertTrue(rows.all { it.mono })
    }

    @Test
    fun theDominantLanguageWhenThePlayingFileHasNone() {
        val episodes = listOf(ep(1, 1, "a", 0, language = null), ep(1, 2, "b", 0, language = "french"), ep(1, 3, "c", 0, language = "french"))
        assertEquals("french", currentLanguage(episodes[0], episodes))
    }
}
