package studio.kahn.iris.tv.ui.screens.home

import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import studio.kahn.iris.tv.data.CatalogCard
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.screens.home.HomeWordsTest.Companion.cw
import studio.kahn.iris.tv.ui.state.Loadable

/** API items to home and discover models: what each card and the hero say, and what they lead to. */
// Robolectric: language names go through Media3's code normalisation, which reads android.*.
@RunWith(RobolectricTestRunner::class)
class HomeModelsTest {
    private val series = UUID.fromString("00000000-0000-0000-0000-000000000001")

    @Test
    fun aResumedFilmHasNoEpisodes() {
        val film = UUID.fromString("00000000-0000-0000-0000-000000000002")
        val item = cw(position = 1930.0, duration = 3310.0, collection = film, kind = MediaKind.movie, name = "Avatar.2009.MULTi.1080p.BluRay.mkv")
        val hero = resumeHero(item, null, PlaybackPrefsResponse(audioLanguage = "fr", subtitleLanguage = "off", forCollection = true))
        assertEquals(listOf("Resume at 32:10", "Start over"), hero.actions.map { it.label })
        assertEquals("Plays with audio in French, subtitles off, as chosen for this film.", hero.languages)
        assertTrue(CardAction.OpenSeries !in continueCard(item, null).menu)
    }

    @Test
    fun aResumedEpisode() {
        val item = cw(position = 1930.0, duration = 3310.0, season = 2, episode = 4, collection = series, kind = MediaKind.tv)
            .copy(episodeName = "Woe's Hollow")
        val card = continueCard(item, null)
        assertEquals("cw:c:$series", card.key)
        assertEquals("Severance", card.title)
        assertEquals("S2:E4 · 23 min left", card.meta)
        assertNull(card.status)
        assertEquals(CardAction.Play, card.primary)
        assertEquals(
            listOf(CardAction.Play, CardAction.StartOver, CardAction.OpenSeries, CardAction.MarkWatched, CardAction.RemoveFromContinue),
            card.menu,
        )

        val hero = resumeHero(item, null, PlaybackPrefsResponse(audioLanguage = "en"))
        assertEquals("Continue where you left off", hero.eyebrow)
        assertEquals("Season 2 · Episode 4 · Woe's Hollow · 23 min left", hero.meta)
        assertEquals(listOf("Resume at 32:10", "All episodes", "Start over"), hero.actions.map { it.label })
        assertEquals("Plays with audio in English.", hero.languages)
    }

    @Test
    fun aNextEpisodeNotOnDiskIsGotThenPlayed() {
        val item = cw(season = 1, episode = 5, collection = series, grabbable = true, kind = MediaKind.tv)
        val card = continueCard(item, null)
        assertEquals("Up next · S1:E5 · Not downloaded", card.status)
        assertEquals(CardAction.GetAndPlay, card.primary)
        assertFalse(CardAction.MarkWatched in card.menu)
        assertNull(card.progress)

        val hero = resumeHero(item, null, null)
        assertEquals("Up next", hero.eyebrow)
        assertEquals(HeroButton(HeroAction.GetAndPlay, "Play S1:E5", "Getting S1:E5…", "get:c:$series"), hero.actions.first())
    }

    @Test
    fun aMovieJustStarted() {
        val item = cw(position = 2.0, duration = 6000.0, name = "Perfect.Days.2023.1080p.mkv")
        assertEquals("Movie · 1 h 40 min left", continueCard(item, null).meta)
        val hero = resumeHero(item, null, null)
        assertEquals(listOf("Play"), hero.actions.map { it.label })
        assertEquals("Movie · 1 h 40 min", hero.meta)
        assertEquals("Perfect Days (2023)", hero.title)
    }

    @Test
    fun libraryCardsSayWhereTheFilesAre() {
        val c = CollectionListItem("Severance", 9, series, MediaKind.tv, 2, 40L * 1024 * 1024 * 1024)
        assertEquals("Series · 9 episodes", libraryCard(c, null).meta)
        assertEquals("On disk" to StatusTone.Ok, libraryCard(c, null).let { it.status to it.tone })
        assertEquals("Downloading · 42%", libraryCard(c, 42.4).status)
        assertEquals("No longer on disk" to StatusTone.Warn, libraryCard(c.copy(ghost = true), null).let { it.status to it.tone })
    }

    @Test
    fun suggestionsLeadToASearch() {
        val card = CatalogCard(
            alreadyInLibrary = false,
            availability = "available",
            catalogId = series,
            isAnime = true,
            kind = MediaKind.tv,
            title = "Frieren",
            reason = "Because you like Fantasy",
            seeders = 1,
            year = 2023,
        )
        val m = catalogCard(card, "fy:trending:")
        assertEquals("fy:trending:$series", m.key)
        assertEquals("Anime · Series · 2023 · 1 seeder", m.meta)
        assertEquals("Because you like Fantasy", m.status)
        assertEquals(CardAction.FindReleases, m.primary)
        assertEquals("In your library", catalogCard(card.copy(alreadyInLibrary = true), "x").status)
    }

    @Test
    fun theHeroWaitsForContinueWatchingThenFallsBackToTheLibrary() {
        val c = CollectionListItem("Severance", 9, series, MediaKind.tv, 2, 1)
        val waiting = homeUi(HomeData(collections = Loadable.Ready(listOf(c))))
        assertNull(waiting.hero)
        assertTrue(waiting.heroPending)

        val library = homeUi(HomeData(continueWatching = Loadable.Ready(emptyList()), collections = Loadable.Ready(listOf(c))))
        assertEquals("In your library", library.hero?.eyebrow)
        assertFalse(library.heroPending)

        val nothing = homeUi(HomeData(continueWatching = Loadable.Ready(emptyList()), collections = Loadable.Ready(emptyList())))
        assertNull(nothing.hero)
        assertFalse(nothing.heroPending)
    }

    @Test
    fun onboardingIsDueUntilDoneOrPutOff() {
        val prefs = studio.kahn.iris.tv.data.PreferencesResponse(emptyList(), false, emptyList(), onboardingCompleted = false)
        assertEquals(prefs, homeUi(HomeData(preferences = prefs)).onboarding)
        assertNull(homeUi(HomeData(preferences = prefs, onboardingClosed = true)).onboarding)
        assertNull(homeUi(HomeData(preferences = prefs.copy(onboardingCompleted = true))).onboarding)
    }

    @Test
    fun aMoodSaysItsNameAndCount() {
        val d = DiscoverData(
            kind = MediaKind.tv,
            mood = "feel-good",
            boards = mapOf(
                MediaKind.tv to Loadable.Ready(studio.kahn.iris.tv.data.MoodBoard(listOf(studio.kahn.iris.tv.data.MoodTile("feel-good", "Feel-good", featuredTitle = "Ted Lasso")))),
            ),
            results = mapOf(("feel-good" to MediaKind.tv) to Loadable.Ready(studio.kahn.iris.tv.data.MoodResults(emptyList(), "tv", "feel-good"))),
        )
        val ui = discoverUi(d)
        assertEquals("Now: Ted Lasso", ui.board.valueOrNull?.single()?.now)
        assertEquals("Feel-good", ui.mood?.label)
        assertEquals("0 series", ui.mood?.count)
    }

    @Test
    fun toggling() {
        assertEquals(listOf("fr", "en"), listOf("fr").toggled("en"))
        assertEquals(listOf("fr"), listOf("fr", "en").toggled("en"))
    }
}
