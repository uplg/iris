package studio.kahn.iris.tv.ui.screens.library

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.screenshot.LibraryFixtures as F
import studio.kahn.iris.tv.ui.components.StatusTone

class LibraryModelTest {
    private val activity = activityByCollection(F.torrents)

    @Test
    fun countsLeaveGhostsOut() {
        val c = titleCounts(F.titles)
        assertEquals(13, c.total)
        assertEquals("5 movies · 7 series · 1 anime", c.text)
    }

    @Test
    fun titleStatusSaysWhatIsHappening() {
        val bear = F.titles.first { it.displayTitle == "The Bear" }
        assertEquals(Status(StatusTone.Busy, "Downloading S4 · 42%"), titleStatus(bear, activity[bear.id.toString()]))
        val arcane = F.titles.first { it.displayTitle == "Arcane" }
        assertEquals(Status(StatusTone.Warn, "Download stuck · 61%"), titleStatus(arcane, activity[arcane.id.toString()]))
        val ghost = F.titles.first { it.ghost == true }
        assertEquals(Status(StatusTone.Info, "No longer on disk"), titleStatus(ghost, null))
        val severance = F.titles.first()
        assertEquals("19 episodes on disk", titleStatus(severance.copy(watch = null), null).text)
        assertEquals("On disk", titleStatus(F.titles[1].copy(watch = null), null).text)
    }

    @Test
    fun filtersCombineAndSort() {
        val series = filterTitles(F.titles, TitleFilters(type = TypeFilter.Series), activity)
        assertTrue(series.all { it.kind == MediaKind.tv && it.isAnime != true })
        assertEquals(listOf("Frieren"), filterTitles(F.titles, TitleFilters(type = TypeFilter.Anime), activity).map { it.displayTitle })
        assertEquals(listOf("The Bear", "Arcane"), filterTitles(F.titles, TitleFilters(show = ShowFilter.Downloading), activity).map { it.displayTitle })
        assertEquals(listOf("Past Lives"), filterTitles(F.titles, TitleFilters(show = ShowFilter.Gone), activity).map { it.displayTitle })
        assertEquals(listOf("Dune: Part Two"), filterTitles(F.titles, TitleFilters(query = "  DUNE "), activity).map { it.displayTitle })
        val bySize = filterTitles(F.titles, TitleFilters(sort = Sort.Size), activity)
        assertEquals("Andor", bySize.first().displayTitle)
        val byTitle = filterTitles(F.titles, TitleFilters(sort = Sort.Title), activity)
        assertEquals("Aftersun", byTitle.first().displayTitle)
        assertEquals(F.titles, filterTitles(F.titles, TitleFilters(), activity))
    }

    @Test
    fun showChoicesOfferOnlyStatesInUse() {
        assertEquals(
            listOf("Everything", "Downloading (2)", "No longer on disk (1)"),
            showChoices(F.titles, activity).map { it.second },
        )
        assertEquals(listOf("Everything"), showChoices(F.titles.filter { it.ghost != true }, emptyMap()).map { it.second })
    }

    @Test
    fun releasesGroupByWhatTheyDo() {
        val groups = F.torrents.associate { it.infohash to groupOf(it) }
        assertEquals(ReleaseGroup.Downloading, groups["aa01"])
        assertEquals(ReleaseGroup.Attention, groups["aa02"])
        assertEquals(ReleaseGroup.Seeding, groups["aa03"])
        assertEquals(ReleaseGroup.Attention, groups["aa05"])
    }

    @Test
    fun releaseStatusInWords() {
        assertEquals("Downloading · 42% · 6.1 MB/s · 18 peers · about 13 min", releaseStatus(F.torrents[0]).text)
        assertEquals("Stalled · no peers · 61%", releaseStatus(F.torrents[1]).text)
        assertEquals("Seeding · 3 peers downloading · 117 KB/s up", releaseStatus(F.torrents[2]).text)
        assertEquals("Paused after download · nyaa releases never seed", releaseStatus(F.torrents[4]).text)
        val broken = F.torrent("x", "X", state = TorrentState.error).copy(error = "disk full")
        assertEquals(Status(StatusTone.Warn, "Error · disk full"), releaseStatus(broken))
    }

    @Test
    fun pauseAndResumeFollowTheState() {
        assertTrue(canPause(F.torrents[0]))
        assertTrue(canResume(F.torrents[4]))
        assertEquals("Only an admin or Camille can do this", noDeleteReason(F.torrents[1]))
    }

    @Test
    fun deleteNamesTheFiles() {
        val text = deleteDescription(F.torrents[3])
        assertTrue(text, text.startsWith("This removes 10 files (Severance.S02E01.1080p.mkv, Severance.S02E02.1080p.mkv, Severance.S02E03.1080p.mkv and 7 more)"))
    }

    @Test
    fun seasonOfReadsTheName() {
        assertEquals("S4", seasonOf("The.Bear.S04.1080p"))
        assertEquals("S1:E3", seasonOf("Show.S01E03.720p"))
        assertNull(seasonOf("Dune.Part.Two.2024"))
    }

    @Test
    fun libraryFactsLine() {
        assertEquals("13 titles · 412 GB free of 2.0 TB · 2 downloading · 3 seeding", libraryFacts(titleCounts(F.titles), F.summary))
        assertNull(libraryFacts(null, null))
    }

    @Test
    fun titlesUiSaysWhereThePersonIs() {
        val ui = titlesUi(F.titles, F.torrents, emptyList(), TitleFilters())
        fun card(title: String) = ui.cards.first { it.title == title }
        assertEquals("In progress · 1 h 23 min left", card("Perfect Days").status.text)
        assertEquals(2_400f / 7_400f, card("Perfect Days").progress!!, 0.001f)
        assertEquals(Status(StatusTone.Ok, "In progress · S2:E4 · 30 min left"), card("Severance").status)
        assertEquals(Status(StatusTone.Info, "Watched"), card("Fallout").status)
        assertTrue(card("Fallout").watched)
        assertNull(card("Fallout").progress)
        assertEquals("Last watched S3:E2", card("Slow Horses").status.text)
        assertEquals(StatusTone.Busy, card("The Bear").status.tone)
        assertNull(card("The Bear").progress)
        assertEquals("14 titles", ui.countWords)
    }
}
