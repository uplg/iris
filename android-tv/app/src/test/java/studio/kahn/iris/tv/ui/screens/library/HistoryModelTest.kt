package studio.kahn.iris.tv.ui.screens.library

import java.time.ZoneOffset
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.screenshot.LibraryFixtures as F

class HistoryModelTest {
    @Test
    fun groupsByTitleInTheOrderWatched() {
        val groups = groupHistory(F.history)
        assertEquals(listOf("Severance", "Past Lives", "Dune: Part Two"), groups.map { it.title })
        assertFalse(groups[0].solo)
        assertEquals(2, groups[0].items.size)
        assertTrue(groups[1].solo)
        assertTrue(groups[1].ghost)
        assertFalse(groups[2].ghost)
    }

    @Test
    fun aGoneReleaseWithItsSourceCanComeBack() {
        val ui = historyUi(F.history, F.now, ZoneOffset.UTC)
        assertEquals(LineAction.Restore, ui[1].lines[0].action)
        assertEquals(LineAction.Play, ui[0].lines[0].action)
        val noSource = F.history[2].copy(sourceProvider = null)
        assertEquals(LineAction.OpenTitle, historyUi(listOf(noSource), F.now)[0].lines[0].action)
    }

    @Test
    fun wordsForHowFarAndWhen() {
        assertEquals("Watched to the end", progressWords(10.0, 20.0, completed = true))
        assertEquals("Not started", progressWords(0.0, 20.0, completed = false))
        assertEquals("40% watched, stopped at 20:00", progressWords(1_200.0, 3_000.0, completed = false))
        assertEquals("Stopped at 1:00:00", progressWords(3_600.0, null, completed = false))
        assertEquals("S2:E4", whatWatched(F.history[0]))
        assertEquals(
            "40% watched, stopped at 20:00 · Last watched today at 18:00",
            historyFacts(F.history[0], F.now, ZoneOffset.UTC),
        )
        assertEquals(0.4f, watchedShare(1_200.0, 3_000.0, false), 0.0001f)
    }
}
