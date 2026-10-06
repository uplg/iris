package studio.kahn.iris.tv.ui.screens.search

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.ProviderResultMeta
import studio.kahn.iris.tv.data.SearchResponse
import studio.kahn.iris.tv.data.SearchResult

@OptIn(ExperimentalCoroutinesApi::class)
class ReleasePagerTest {
    private fun r(id: String, tag: String? = null, seeders: Int? = 10) =
        SearchResult(externalId = id, providerId = "v3x", title = "Show.S01.1080p", languageTag = tag, seeders = seeders)

    private fun page(vararg rows: SearchResult) = SearchResponse(
        providers = listOf(ProviderResultMeta(currentPage = 1, id = "v3x", limit = 25, totalPages = 9)),
        results = rows.toList(),
        libraryMatches = emptyList(),
    )

    private val pages = mapOf(
        1 to page(r("1", "fr"), r("2", "en")),
        2 to page(r("3", "en"), r("4", "en")),
        3 to page(r("5", "fr")),
    )
    private val asked = mutableListOf<Int>()

    private fun TestScope.pager(language: String?) =
        ReleasePager(this, shown = { p -> filterLanguage(p.rows, language).size }) { n ->
            asked += n
            pages.getValue(n)
        }

    @Test
    fun theRuleIsSomethingNewShown() {
        assertTrue(keepsLoadingByItself(shownBefore = 3, shownAfter = 4))
        assertFalse(keepsLoadingByItself(shownBefore = 3, shownAfter = 3))
    }

    @Test
    fun aPageThatShowsNothingNewStopsTheAutomaticPaging() = runTest {
        val pager = pager(language = "fr")
        pager.first()
        runCurrent()
        pager.more()
        runCurrent()
        // Page 2 is all English: nothing new under the French filter.
        assertTrue(pager.state.value.waitsForViewer)
        assertFalse(pager.state.value.autoLoads)
        pager.more()
        runCurrent()
        assertEquals(listOf(1, 2), asked)
        // The viewer asks: page 3 comes, and shows a French release, so paging goes on by itself.
        pager.more(byViewer = true)
        runCurrent()
        assertEquals(listOf(1, 2, 3), asked)
        assertFalse(pager.state.value.waitsForViewer)
    }

    @Test
    fun withoutAFilterEveryPageIsNew() = runTest {
        val pager = pager(language = null)
        pager.first()
        runCurrent()
        pager.more()
        runCurrent()
        assertFalse(pager.state.value.waitsForViewer)
        assertEquals(4, pager.state.value.results?.valueOrNull?.rows?.size)
    }

    @Test
    fun anotherFilterMayLoadByItselfAgain() = runTest {
        val pager = pager(language = "fr")
        pager.first()
        runCurrent()
        pager.more()
        runCurrent()
        pager.filtersChanged()
        assertTrue(pager.state.value.autoLoads)
    }

    @Test
    fun holdOkNeverGrabsAnEmptySwarm() {
        assertEquals(HoldAction.Refuse(DEAD_GRAB), holdAction(r("1", seeders = 0)))
        assertEquals(HoldAction.Grab, holdAction(r("1", seeders = null)))
        assertEquals(HoldAction.Grab, holdAction(r("1", seeders = 3)))
        val owned = r("1", seeders = 0).copy(alreadyInLibrary = true, libraryInfohash = "abc", libraryFileIdx = 2)
        assertEquals(HoldAction.Play(OwnedFile("abc", 2)), holdAction(owned))
    }
}
