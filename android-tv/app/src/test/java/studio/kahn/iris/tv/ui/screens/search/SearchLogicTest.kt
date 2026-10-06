package studio.kahn.iris.tv.ui.screens.search

import java.time.OffsetDateTime
import java.time.ZoneOffset
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import studio.kahn.iris.tv.data.LibraryMatch
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.ParsedQueryInfo
import studio.kahn.iris.tv.data.ProviderResultMeta
import studio.kahn.iris.tv.data.SearchResponse
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.SearchViewMode
import studio.kahn.iris.tv.data.TitleCard
import studio.kahn.iris.tv.data.TitleMatch

class SearchLogicTest {
    private fun r(id: String, provider: String = "torr9", tag: String? = null, title: Long? = null) = SearchResult(
        externalId = id,
        providerId = provider,
        title = "Show.S01.1080p",
        languageTag = tag,
        titleMatch = title?.let { TitleMatch(kind = MediaKind.tv, title = "Show", tmdbId = it) },
    )

    private fun meta(id: String, pages: Int? = null, error: String? = null) =
        ProviderResultMeta(currentPage = 1, id = id, limit = 25, error = error, totalPages = pages)

    @Test
    fun sortsAskTheTrackersExceptBestMatch() {
        assertNull(SearchSort.BestMatch.sortBy)
        assertEquals("seeders" to "desc", SearchSort.Seeders.sortBy to SearchSort.Seeders.order)
        assertEquals("size" to "asc", SearchSort.Smallest.sortBy to SearchSort.Smallest.order)
        assertEquals(SearchSort.Newest, SearchSort.of("Newest"))
        assertEquals(SearchSort.BestMatch, SearchSort.of("something new"))
        assertEquals(listOf(null, "movie", "tv"), SearchKind.entries.map { it.apiKind })
        assertEquals(listOf("Titles", "Grid", "List"), SearchViewMode.entries.map { it.label })
    }

    @Test
    fun nextPageFollowsTheTrackersPages() {
        val two = SearchResponse(providers = listOf(meta("a", 2), meta("b", 1)), results = emptyList(), libraryMatches = emptyList())
        assertEquals(2, nextPage(two, 1))
        assertNull(nextPage(two, 2))
        val unknown = SearchResponse(providers = listOf(meta("a")), results = List(SEARCH_PAGE_SIZE) { r("$it") }, libraryMatches = emptyList())
        assertEquals(3, nextPage(unknown, 2))
        assertNull(nextPage(unknown.copy(results = listOf(r("1"))), 2))
    }

    @Test
    fun mergeKeepsAReleaseOnceAcrossPagesAndTrackers() {
        val merged = mergeResults(listOf(r("1"), r("2")), listOf(r("2"), r("3"), r("1", provider = "v3x")))
        assertEquals(listOf("torr9:1", "torr9:2", "torr9:3", "v3x:1"), merged.map(::releaseKey))
    }

    @Test
    fun audioFilterCountsWhatIsLoaded() {
        val rows = listOf(r("1", tag = "fr"), r("2", tag = "fr"), r("3", tag = "en"), r("4"))
        val options = audioOptions(rows, selected = "vo")
        assertEquals(listOf(null, "fr", "en", "vo"), options.map { it.tag })
        assertEquals(listOf(4, 2, 1, 0), options.map { it.count })
        assertEquals("French (VF) · 2", options[1].words)
        assertEquals(2, filterLanguage(rows, "fr").size)
        assertEquals(rows, filterLanguage(rows, null))
    }

    @Test
    fun summarySaysWhoAnswered() {
        val providers = listOf(meta("torr9"), meta("v3x"), meta("c411", error = "timeout"))
        assertEquals("1 match in your library · 24 releases from 2 trackers · c411 did not answer", summary(1, 24, providers))
        assertEquals("1 release from 1 tracker", summary(0, 1, listOf(meta("torr9"))))
        assertEquals(listOf("c411"), failedTrackers(providers).map { it.id })
        assertEquals("Page 1 of 3", pageWords(1, listOf(meta("a", 3))))
        assertEquals("Page 2", pageWords(2, listOf(meta("a"))))
    }

    @Test
    fun parsedQueryInWords() {
        assertEquals(
            "Showing results for Classroom of the Elite · Season 4, episode 11.",
            parsedWords(ParsedQueryInfo(title = "Classroom of the Elite", season = 4, episode = 11)),
        )
        assertEquals("Showing results for Dune · 2024.", parsedWords(ParsedQueryInfo(title = "Dune", year = 2024)))
        assertNull(parsedWords(null))
    }

    @Test
    fun recentSearchTimes() {
        val now = ZonedDateTime.of(2026, 10, 6, 21, 0, 0, 0, ZoneOffset.UTC)
        fun at(days: Long) = OffsetDateTime.of(2026, 10, 6, 20, 41, 0, 0, ZoneOffset.UTC).minusDays(days)
        assertEquals("Today, 20:41", recentWhen(at(0), now))
        assertEquals("Yesterday", recentWhen(at(1), now))
        assertEquals("Saturday", recentWhen(at(3), now))
        assertEquals("28 Sep", recentWhen(at(8), now))
    }

    @Test
    fun titlesSayWhatIsKnown() {
        val card = TitleCard(kind = MediaKind.tv, title = "Show", tmdbId = 7, year = 2022)
        val counts = releasesByTitle(listOf(r("1", title = 7), r("2", title = 7), r("3", title = 8), r("4")))
        assertEquals(mapOf(7L to 2, 8L to 1), counts)
        assertEquals("2 releases", titleStatus(card, counts))
        assertNull(titleStatus(card, emptyMap()))
        assertEquals("In your library", titleStatus(card.copy(collectionId = java.util.UUID(0, 1)), counts))
        assertEquals("Series · 2022", titleMeta(card))
    }

    @Test
    fun seasonsOfferedOnlyWhenThereAreSeveral() {
        fun s(id: String, season: Int?) = r(id).copy(parsedSeason = season)
        assertTrue(seasonOptions(listOf(s("1", 1), s("2", 1)), null).isEmpty())
        assertEquals(
            listOf("All seasons", "Season 1", "Season 2"),
            seasonOptions(listOf(s("1", 2), s("2", 1), s("3", null)), null).map { it.label },
        )
    }

    @Test
    fun libraryMatchLeadsToTheEpisodeOrTheCollection() {
        val m = LibraryMatch(collectionId = "c", displayTitle = "Show", episodeCount = 9, isAnime = false, kind = "tv", torrentCount = 1)
        assertEquals(MatchTarget.Open("c", "9 episodes on disk"), matchTarget(m))
        assertEquals(MatchTarget.Open("c", "Season 3: 4 episodes on disk"), matchTarget(m.copy(episodeSeason = 3, seasonEpisodeCount = 4)))
        val play = matchTarget(m.copy(episodeInfohash = "abc", episodeFileIdx = 2, episodeSeason = 3, episodeNumber = 4))
        assertEquals(MatchTarget.Play("abc", 2, "Play S3:E4", "The episode you asked for is on disk"), play)
        assertEquals("2 releases on disk", matchTarget(m.copy(kind = "movie", torrentCount = 2)).facts)
    }
}
