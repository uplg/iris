package studio.kahn.iris.tv.screenshot

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.data.GenreOption
import studio.kahn.iris.tv.data.LanguageOption
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.ui.components.StatusTone
import studio.kahn.iris.tv.ui.screens.DiscoverContent
import studio.kahn.iris.tv.ui.screens.HomeContent
import studio.kahn.iris.tv.ui.screens.OnboardingContent
import studio.kahn.iris.tv.ui.screens.home.CardAction
import studio.kahn.iris.tv.ui.screens.home.CardMenu
import studio.kahn.iris.tv.ui.screens.home.CardModel
import studio.kahn.iris.tv.ui.screens.home.DiscoverUiState
import studio.kahn.iris.tv.ui.screens.home.HeroAction
import studio.kahn.iris.tv.ui.screens.home.HeroButton
import studio.kahn.iris.tv.ui.screens.home.HeroModel
import studio.kahn.iris.tv.ui.screens.home.HomeUiState
import studio.kahn.iris.tv.ui.screens.home.MoodModel
import studio.kahn.iris.tv.ui.screens.home.MoodResultsModel
import studio.kahn.iris.tv.ui.screens.home.Notice
import studio.kahn.iris.tv.ui.screens.home.OnboardingUiState
import studio.kahn.iris.tv.ui.screens.home.Picks
import studio.kahn.iris.tv.ui.screens.home.ShelfModel
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError

/** Home, Discover and the first-run sheet, from fake states (TV.dc.html), at every size. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class HomeScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    private val hero = HeroModel(
        key = "resume:c:1",
        eyebrow = "Continue where you left off",
        title = "Severance",
        meta = "Season 2 · Episode 4 · Woe's Hollow · 23 min left",
        overview = "Mark and his team push deeper into Lumon while their outies start asking questions of their own.",
        art = null,
        actions = listOf(
            HeroButton(HeroAction.Resume, "Resume at 32:10"),
            HeroButton(HeroAction.AllEpisodes, "All episodes"),
            HeroButton(HeroAction.StartOver, "Start over", "Starting over…", "over:c:1"),
        ),
        languages = "Plays with audio in English, subtitles in French, as chosen for this series.",
    )

    private fun cw(key: String, title: String, meta: String, progress: Float?, status: String? = null) = CardModel(
        key = "cw:$key",
        title = title,
        art = null,
        meta = meta,
        status = status,
        progress = progress,
        primary = CardAction.Play,
        menu = listOf(CardAction.Play, CardAction.MarkWatched, CardAction.RemoveFromContinue),
    )

    private val continueWatching = listOf(
        cw("c:1", "Severance", "S2:E4 · 23 min left", 0.58f),
        cw("c:2", "Shōgun", "S1:E7 · 41 min left", 0.22f),
        cw("m:1", "Perfect Days", "Movie · 1 h 12 min left", 0.42f),
        cw("c:3", "Frieren", "E20", null, "Up next · E20 · Not downloaded"),
    )

    private fun poster(prefix: String, i: Int, title: String, status: String, tone: StatusTone = StatusTone.Muted) = CardModel(
        key = "$prefix$i",
        title = title,
        art = null,
        kind = "Series",
        meta = "Series",
        status = status,
        tone = tone,
        menu = listOf(CardAction.Open),
    )

    private val watchlist = listOf(
        poster("wl:", 1, "The Bear", "3 new episodes", StatusTone.Ok),
        poster("wl:", 2, "Slow Horses", "Downloading · 42%"),
        poster("wl:", 3, "Andor", "No new episodes"),
        poster("wl:", 4, "Pachinko", "No new episodes"),
        poster("wl:", 5, "Dune: Prophecy", "1 new episode", StatusTone.Ok),
        poster("wl:", 6, "Silo", "No new episodes"),
        poster("wl:", 7, "Fallout", "No new episodes"),
        poster("wl:", 8, "Blue Eye Samurai", "No new episodes"),
    )

    private val home = HomeUiState(
        account = "Leonard",
        hero = hero,
        heroPending = false,
        rightNow = Loadable.Ready(listOf("2 downloads · 64% · 23 min left", "3 new episodes on your watchlist", "412 GB free on disk")),
        continueWatching = Loadable.Ready(continueWatching),
        watchlist = Loadable.Ready(watchlist),
        watchlistCount = watchlist.size,
        forYou = listOf(
            ShelfModel(
                "fy:trending:",
                "Trending this week",
                (1..8).map { poster("fy:trending:", it, "Title $it", "In your library", StatusTone.Ok) },
            ),
        ),
        library = Loadable.Ready((1..8).map { poster("lib:", it, "Library $it", "On disk", StatusTone.Ok) }),
        libraryCount = 64,
    )

    @Composable
    private fun Home(state: HomeUiState) {
        HomeContent(
            state = state,
            onSelectTab = {},
            onAccount = {},
            onHeroAction = {},
            onCardAction = { _, _ -> },
            onRetry = {},
            onOpenDiscover = {},
            onOpenLibrary = {},
            onOpenSearch = {},
        )
    }

    @Test
    fun home() = shots.snapEverySize("home") { Home(home) }

    @Test
    fun homeNotice() = shots.snap("home_notice") {
        Home(home.copy(notice = Notice("Could not get E20. The server is busy. Open the series to pick another release.", StatusTone.Down), updateAvailable = true))
    }

    @Test
    fun homeEmpty() = shots.snap("home_empty") {
        Home(
            HomeUiState(
                account = "Leonard",
                heroPending = false,
                rightNow = Loadable.Failed(UiError(UiError.OFFLINE_MESSAGE, code = UiError.NETWORK)),
                continueWatching = Loadable.Ready(emptyList()),
                watchlist = Loadable.Failed(UiError(UiError.OFFLINE_MESSAGE, code = UiError.NETWORK)),
                library = Loadable.Loading,
            ),
        )
    }

    @Test
    fun cardMenu() = shots.snap("home_card_menu") {
        Box {
            Home(home)
            CardMenu(
                eyebrow = "Continue watching",
                title = "Severance",
                actions = listOf(CardAction.Play, CardAction.StartOver, CardAction.OpenSeries, CardAction.MarkWatched, CardAction.RemoveFromContinue),
                busy = CardAction.MarkWatched,
                onAction = {},
                onDismiss = {},
            )
        }
    }

    @Test
    fun onboarding() = shots.snapEverySize("onboarding") {
        Box {
            Home(home)
            OnboardingContent(
                state = OnboardingUiState(
                    picks = Picks(languages = listOf("fr"), genres = listOf(18), includeAnime = true),
                    languages = Loadable.Ready(listOf(LanguageOption("French", "fr"), LanguageOption("English", "en"), LanguageOption("Japanese", "ja"))),
                    genres = Loadable.Ready(
                        listOf("Action", "Comedy", "Drama", "Documentary", "Science Fiction", "Thriller", "Crime", "Family", "Animation", "Horror")
                            .mapIndexed { i, name -> GenreOption(if (name == "Drama") 18 else i.toLong(), name) },
                    ),
                ),
                onToggleLanguage = {},
                onToggleGenre = {},
                onToggleAnime = {},
                onSave = {},
                onSkip = {},
                onRetry = {},
                onDismiss = {},
            )
        }
    }

    private val moods = listOf("Feel-good", "Edge of your seat", "Mind-bending", "Laugh out loud", "Cosy night in", "Epic journeys", "True stories", "Animated")
        .mapIndexed { i, label -> MoodModel("m$i", label, null, "Now: Title ${i + 1}") }

    private val discover = DiscoverUiState(
        account = "Leonard",
        kind = MediaKind.movie,
        board = Loadable.Ready(moods),
        forYou = Loadable.Ready(home.forYou),
    )

    @Composable
    private fun Discover(state: DiscoverUiState) {
        DiscoverContent(
            state = state,
            onSelectTab = {},
            onAccount = {},
            onKind = {},
            onOpenMood = {},
            onCloseMood = {},
            onCardAction = { _, _ -> },
            onRetry = {},
        )
    }

    @Test
    fun discover() = shots.snapEverySize("discover") { Discover(discover) }

    @Test
    fun discoverMood() = shots.snapEverySize("discover_mood") {
        Discover(
            discover.copy(
                kind = MediaKind.tv,
                mood = MoodResultsModel(
                    id = "m0",
                    label = "Feel-good",
                    count = "9 series",
                    cards = Loadable.Ready(
                        (1..9).map {
                            CardModel(
                                key = "mood:$it",
                                title = "Series $it",
                                art = null,
                                kind = "Series",
                                meta = "Series · 2024 · 12 seeders",
                                status = if (it % 3 == 0) "In your library" else "Because you like Comedy",
                                tone = if (it % 3 == 0) StatusTone.Ok else StatusTone.Muted,
                                primary = CardAction.FindReleases,
                                menu = listOf(CardAction.FindReleases, CardAction.NotInterested),
                            )
                        },
                    ),
                ),
            ),
        )
    }
}
