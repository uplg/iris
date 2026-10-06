package studio.kahn.iris.tv.ui.screens.player

import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.EpisodePoint
import studio.kahn.iris.tv.data.EpisodeStatus
import studio.kahn.iris.tv.ui.format.episodeCode

class ErrorStepTest {
    @Test
    fun aDecoderErrorOnTheServersOwnStreamIsSaidNotRemuxedAgain() {
        // The transcode's URL is the remux's: "falling back" kept the same dead player, silent.
        assertEquals(ErrorStep.Say, errorStep(transient = false, retriesLeft = true, remuxable = true, route = PlayRoute.ServerTranscode))
        assertEquals(ErrorStep.Say, errorStep(transient = false, retriesLeft = true, remuxable = true, route = PlayRoute.ServerRemux))
        assertEquals(ErrorStep.Remux, errorStep(transient = false, retriesLeft = true, remuxable = true, route = PlayRoute.Direct))
    }

    @Test
    fun aNetworkBlipRetriesWhileRetriesAreLeft() {
        assertEquals(ErrorStep.Retry, errorStep(transient = true, retriesLeft = true, remuxable = false, route = PlayRoute.ServerRemux))
        assertEquals(ErrorStep.Say, errorStep(transient = true, retriesLeft = false, remuxable = false, route = PlayRoute.Direct))
    }
}

class SeekAccelerationTest {
    @Test
    fun aPressMovesTenSeconds() {
        assertEquals(10_000L, SeekAcceleration.offsetMs(0))
        assertEquals(10_000L, SeekAcceleration.offsetMs(300))
        assertEquals(10_000L, SeekAcceleration.offsetMs(500))
    }

    @Test
    fun holdingGoesFasterTheLongerItIsHeld() {
        // 0.5 s → 2.5 s at 20 s per held second: +40 s.
        assertEquals(50_000L, SeekAcceleration.offsetMs(2_500))
        // then 60 s per second up to 6.5 s: +240 s.
        assertEquals(290_000L, SeekAcceleration.offsetMs(6_500))
        // then 180 s per second.
        assertEquals(470_000L, SeekAcceleration.offsetMs(7_500))
        var last = 0L
        for (held in 0L..20_000L step 250) {
            val offset = SeekAcceleration.offsetMs(held)
            assertTrue("monotonic at $held", offset >= last)
            last = offset
        }
    }

    @Test
    fun theTargetStaysInsideTheFile() {
        assertEquals(0L, SeekAcceleration.target(4_000, forward = false, heldMs = 0, durationMs = 60_000))
        assertEquals(60_000L, SeekAcceleration.target(55_000, forward = true, heldMs = 0, durationMs = 60_000))
        assertEquals(40_000L, SeekAcceleration.target(30_000, forward = true, heldMs = 100, durationMs = 60_000))
        // An unknown duration only clamps at the start.
        assertEquals(1_010_000L, SeekAcceleration.target(1_000_000, forward = true, heldMs = 0, durationMs = 0))
    }
}

class PlayerWordsTest {
    @Test
    fun clockFigures() {
        assertEquals("0:00", clockText(0))
        assertEquals("32:10", clockText(1_930_000))
        assertEquals("1:02:03", clockText(3_723_000))
        assertEquals("−22:50", remainingText(1_930_000, 3_300_000))
    }

    @Test
    fun episodeCodesAndNames() {
        assertEquals("S2:E4", episodeCode(2, 4))
        assertEquals("Season 2 · Episode 4 · Woe's Hollow", episodeLine(point(EpisodeStatus.downloaded, name = "Woe's Hollow", episode = 4)))
        assertEquals("Season 2 · Episode 4", episodeLine(point(EpisodeStatus.downloaded, name = " ", episode = 4)))
        assertNull(episodeLine(null))
    }

    @Test
    fun releaseNamesReadable() {
        assertEquals("Severance", prettyName("Season 2/Severance.S02E04.1080p.mkv"))
        assertEquals("Mercato (2025)", prettyName("Mercato.2025.1080p.WEB.x264-GRP.mkv"))
        assertNull(prettyName(""))
    }

    @Test
    fun retrySearchUsesTheSeriesAndEpisode() {
        assertEquals("Severance S02E04", retrySearchQuery("Severance", 2, 4, "x"))
        assertEquals("Dune", retrySearchQuery("Dune", null, null, "x"))
        assertEquals("Dune Part Two 2024", retrySearchQuery(null, null, null, "Dune.Part.Two.2024.mkv"))
    }

    @Test
    fun watchedAndNearEnd() {
        assertFalse(isWatched(80, 100))
        assertTrue(isWatched(90, 100))
        assertFalse(isWatched(90, null))
        assertFalse(isNearEnd(94, 100))
        assertTrue(isNearEnd(95, 100))
        assertFalse(isNearEnd(95, 0))
    }
}

class NextEpisodeRuleTest {
    private val follow: UUID = UUID.fromString("00000000-0000-0000-0000-000000000001")

    @Test
    fun nextIsOfferedNearTheEndWhenOnDisk() {
        val next = point(EpisodeStatus.downloaded, infohash = "ab", fileIdx = 3, name = "Trojan's Horse")
        assertEquals("Next: Trojan's Horse", NextEpisodeRule.onDisk(next)?.label)
        assertFalse(NextEpisodeRule.offersNext(next, isMovie = false, nearEnd = false, ended = false))
        assertTrue(NextEpisodeRule.offersNext(next, isMovie = false, nearEnd = true, ended = false))
        assertTrue(NextEpisodeRule.offersNext(next, isMovie = false, nearEnd = false, ended = true))
        assertFalse(NextEpisodeRule.offersNext(next, isMovie = true, nearEnd = true, ended = true))
    }

    @Test
    fun nextWithoutANameUsesItsCode() {
        assertEquals("Next: S2:E5", NextEpisodeRule.onDisk(point(EpisodeStatus.downloaded, infohash = "ab", fileIdx = 1))?.label)
    }

    @Test
    fun notOnDiskIsNotOffered() {
        val available = point(EpisodeStatus.available, followId = follow)
        assertNull(NextEpisodeRule.onDisk(available))
        assertFalse(NextEpisodeRule.offersNext(available, isMovie = false, nearEnd = true, ended = true))
        assertNull(NextEpisodeRule.onDisk(point(EpisodeStatus.downloaded)))
    }

    @Test
    fun prepareIsAskedOnceForAFollowedSeries() {
        val available = point(EpisodeStatus.available, followId = follow)
        assertTrue(NextEpisodeRule.promptsPrepare(available, followed = true, nearEnd = true, ended = false, alreadyAsked = false))
        assertFalse(NextEpisodeRule.promptsPrepare(available, followed = true, nearEnd = true, ended = false, alreadyAsked = true))
        assertFalse(NextEpisodeRule.promptsPrepare(available, followed = false, nearEnd = true, ended = false, alreadyAsked = false))
        assertFalse(NextEpisodeRule.promptsPrepare(available, followed = true, nearEnd = false, ended = false, alreadyAsked = false))
        val onDisk = point(EpisodeStatus.downloaded, followId = follow, infohash = "ab", fileIdx = 1)
        assertFalse(NextEpisodeRule.promptsPrepare(onDisk, followed = true, nearEnd = true, ended = true, alreadyAsked = false))
    }
}

private fun point(
    status: EpisodeStatus,
    infohash: String? = null,
    fileIdx: Long? = null,
    followId: UUID? = null,
    name: String? = null,
    episode: Long = 5,
) = EpisodePoint(
    episode = episode,
    season = 2,
    status = status,
    fileIdx = fileIdx,
    followId = followId,
    infohash = infohash,
    name = name,
)
