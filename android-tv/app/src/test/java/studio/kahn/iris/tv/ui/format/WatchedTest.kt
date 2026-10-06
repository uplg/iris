package studio.kahn.iris.tv.ui.format

import java.time.OffsetDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.TitleWatch

class WatchedTest {
    private val base = TitleWatch(
        completed = false,
        fileIdx = 3,
        infohash = "aa",
        lastWatchedAt = OffsetDateTime.parse("2026-10-06T10:00:00Z"),
        positionSeconds = 600.0,
        watchedEpisodes = 6,
        durationSeconds = 2_400.0,
        episode = 7,
        season = 1,
    )

    @Test
    fun resumesAFileLeftMidWayWithWhatIsLeft() {
        assertEquals(Resume("aa", 3, "S1:E7", 1_800.0, 0.25f), resumeOf(base))
        assertNull(resumeOf(base.copy(positionSeconds = 2.0)))
        assertNull(resumeOf(base.copy(completed = true)))
        assertNull(resumeOf(null))
        assertEquals(Resume("aa", 3, null, null, null), resumeOf(base.copy(season = null, episode = null, durationSeconds = null)))
    }

    @Test
    fun aSeriesIsWatchedOnceEveryEpisodeOnDiskIs() {
        assertTrue(allWatched(base.copy(watchedEpisodes = 10, completed = true), series = true, episodeCount = 10))
        assertFalse(allWatched(base, series = true, episodeCount = 10))
        assertFalse(allWatched(base.copy(watchedEpisodes = 0), series = true, episodeCount = 0))
        assertTrue(allWatched(base.copy(completed = true), series = false, episodeCount = 1))
        assertFalse(allWatched(null, series = false, episodeCount = 1))
    }

    @Test
    fun putsItInWords() {
        assertEquals("In progress · S1:E7 · 30 min left", watchWords(base, series = true, episodeCount = 10))
        assertEquals("Last watched S1:E7", watchWords(base.copy(completed = true), series = true, episodeCount = 10))
        assertEquals("Watched", watchWords(base.copy(completed = true, watchedEpisodes = 10), series = true, episodeCount = 10))
        assertEquals("In progress", watchWords(base.copy(season = null, episode = null, durationSeconds = null), series = false, episodeCount = 1))
        assertNull(watchWords(null, series = true, episodeCount = 10))
    }

    @Test
    fun theToggleSaysWhatItDoes() {
        assertEquals("Mark as watched", markWatchedLabel(false))
        assertEquals("Mark as not watched", markWatchedLabel(true))
        assertEquals("Severance is marked as watched.", markedWatchedWords("Severance", false))
        assertEquals("Severance is marked as not watched.", markedWatchedWords("Severance", true))
    }
}
