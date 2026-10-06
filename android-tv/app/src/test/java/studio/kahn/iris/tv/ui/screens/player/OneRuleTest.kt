package studio.kahn.iris.tv.ui.screens.player

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.playFileOf
import studio.kahn.iris.tv.ui.format.isResumable
import studio.kahn.iris.tv.ui.screens.library.ReleaseGroup
import studio.kahn.iris.tv.ui.screens.library.WatchState
import studio.kahn.iris.tv.ui.screens.library.groupOf
import studio.kahn.iris.tv.ui.screens.library.stalled

/** The rules every screen shares: when it is a resume, which file plays, when a swarm is stalled. */
class OneRuleTest {
    @Test
    fun aResumeIsFiveSecondsInAndNotFinished() {
        assertFalse(isResumable(null))
        assertFalse(isResumable(4.9))
        assertTrue(isResumable(5.0))
        assertFalse(isResumable(600.0, completed = true))
        assertFalse("a few seconds watched is a Play", WatchState(pct = 0.1, done = false, positionSeconds = 3.0).resumable)
        assertTrue(WatchState(pct = null, done = false, positionSeconds = 600.0).resumable)
        val row = SideRow("k", "ab", 0, "S1:E1", "", mono = false, watched = false, watchedPct = 0.2, active = false, positionSeconds = 2.0)
        assertFalse(row.started)
        assertTrue(row.copy(positionSeconds = 120.0).started)
    }

    @Test
    fun oneFilePlaysAndNeverTheSample() {
        val pack = torrent(
            files = listOf(
                FileEntry(0, "Show/Sample/show.sample.mkv", 9_000),
                FileEntry(1, "Show/Show.S01E02.mkv", 1_000),
                FileEntry(2, "Show/Show.S01E01.mkv", 1_000),
            ),
        )
        assertEquals(2, playFileOf(pack))
        val movie = torrent(files = listOf(FileEntry(0, "Movie/sample.mkv", 10), FileEntry(1, "Movie/movie.mkv", 1_000)))
        assertEquals(1, playFileOf(movie))
        assertNull(playFileOf(torrent(files = listOf(FileEntry(0, "Movie/movie.nfo", 1)))))
    }

    @Test
    fun aSwarmIsStalledOnlyAfterItsGrace() {
        val fresh = torrent(peers = 0, speed = 0, pct = 12.0, ageMinutes = 1)
        assertFalse(stalled(fresh))
        assertEquals("a fresh grab still looking for peers is downloading", ReleaseGroup.Downloading, groupOf(fresh))
        assertFalse(isDeadSwarm(fresh, null))
        val old = torrent(peers = 0, speed = 0, pct = 12.0, ageMinutes = 3)
        assertTrue(stalled(old))
        assertEquals(ReleaseGroup.Attention, groupOf(old))
        assertTrue(isDeadSwarm(old, null))
        assertFalse("a paused release is not a dead swarm", isDeadSwarm(old.copy(state = TorrentState.paused), null))
    }
}
