package studio.kahn.iris.tv.screenshot

import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.data.SearchViewMode
import studio.kahn.iris.tv.ui.components.TopTab
import studio.kahn.iris.tv.ui.nav.TopLevelShell
import studio.kahn.iris.tv.ui.screens.ReleaseActions
import studio.kahn.iris.tv.ui.screens.ReleaseContent
import studio.kahn.iris.tv.ui.screens.ReleasePanel
import studio.kahn.iris.tv.ui.screens.SearchActions
import studio.kahn.iris.tv.ui.screens.SearchContent
import studio.kahn.iris.tv.ui.screens.TitleReleasesActions
import studio.kahn.iris.tv.ui.screens.TitleReleasesContent
import studio.kahn.iris.tv.ui.screens.search.FollowState
import studio.kahn.iris.tv.ui.screens.search.GrabUi
import studio.kahn.iris.tv.ui.screens.search.ReleaseUiState
import studio.kahn.iris.tv.ui.screens.search.SearchUiState
import studio.kahn.iris.tv.ui.screens.search.TitleReleasesUiState
import studio.kahn.iris.tv.ui.screens.search.releaseKey
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.screenshot.SearchFixtures as F

/** Search, its titles, a title's releases and a release, from fake states at every size. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class SearchScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    private val actions = SearchActions(onVoice = {})

    @Composable
    private fun Search(state: SearchUiState, grab: GrabUi = GrabUi.Idle, focus: String? = null) {
        // The on-screen keyboard is for the D-pad: a TV has it, a phone types on its own keyboard.
        val tv = IrisLayout.current.width == 960.dp
        TopLevelShell(TopTab.Search, accountName = "Leonard", onSelect = {}, onAccount = {}) {
            SearchContent(state, grab, onScreenKeyboard = tv, actions = actions, now = F.now, initialFocus = focus)
        }
    }

    private val start = SearchUiState(recent = Loadable.Ready(F.recent))

    private val typed = start.copy(
        typed = "sever",
        query = "sever",
        titles = Loadable.Ready(F.titles),
        results = Loadable.Ready(F.page),
    )

    private val grid = typed.copy(typed = "severance", query = "severance", editing = false, view = SearchViewMode.GRID)

    @Test
    fun start() = shots.snapEverySize("search_start") { Search(start) }

    @Test
    fun titles() = shots.snapEverySize("search_titles") { Search(typed) }

    @Test
    fun grid() = shots.snapEverySize("search_grid") { Search(grid, focus = releaseKey(F.releases[1])) }

    @Test
    fun list() = shots.snapEverySize("search_list") {
        Search(grid.copy(view = SearchViewMode.LIST), focus = releaseKey(F.releases[1]))
    }

    @Test
    fun states() {
        shots.snap("search_loading") { Search(grid.copy(results = Loadable.Loading)) }
        shots.snap("search_failed") {
            Search(grid.copy(results = Loadable.Failed(UiError(UiError.OFFLINE_MESSAGE, code = UiError.NETWORK))))
        }
        shots.snap("search_empty_language") { Search(grid.copy(language = "vo")) }
        shots.snap("search_no_recent") { Search(start.copy(recent = Loadable.Ready(emptyList()))) }
        shots.snap("search_grab_refused") {
            Search(
                grid.copy(view = SearchViewMode.LIST),
                GrabUi.Refused(
                    "1",
                    "`seedpool` allows 1 download at a time. In progress: Dune.Part.Two.2024.2160p (42%). Finish or remove it before grabbing another one.",
                ),
            )
        }
        shots.snap("search_grab_huge") { Search(grid, GrabUi.AskHuge("3", 62_000_000_000L)) }
    }

    @Test
    fun titleReleases() = shots.snapEverySize("title_releases") {
        TitleReleasesContent(
            TitleReleasesUiState(
                query = "severance",
                tmdbId = 95396,
                card = F.titles.first(),
                results = Loadable.Ready(F.page.copy(rows = F.releases.filter { it.titleMatch?.tmdbId == 95396L }, matches = emptyList())),
            ),
            GrabUi.Idle,
            TitleReleasesActions(),
        )
    }

    private val release = ReleaseUiState(
        providerId = "v3x",
        externalId = "2",
        hit = F.releases[1],
        preview = Loadable.Ready(F.preview),
        details = F.details,
        notes = F.notes,
        follow = FollowState.Can(),
    )

    @Test
    fun release() = shots.snapEverySize("release") { ReleaseContent(release, GrabUi.Idle, ReleaseActions(onOtherReleases = { _, _ -> })) }

    @Test
    fun releasePanels() {
        shots.snap("release_notes") { ReleaseContent(release, GrabUi.Idle, ReleaseActions(), initialPanel = ReleasePanel.Notes) }
        shots.snap("release_nfo") { ReleaseContent(release, GrabUi.Idle, ReleaseActions(), initialPanel = ReleasePanel.Nfo) }
        shots.snap("release_files") { ReleaseContent(release, GrabUi.Idle, ReleaseActions(), initialPanel = ReleasePanel.Files) }
    }

    @Test
    fun releaseStates() {
        shots.snap("release_slot_full") {
            ReleaseContent(
                release.copy(
                    preview = Loadable.Failed(
                        UiError(
                            "`seedpool` allows 1 download at a time. In progress: Dune.Part.Two.2024.2160p (42%). Finish or remove it before grabbing another one.",
                            status = 409,
                        ),
                    ),
                ),
                GrabUi.Idle,
                ReleaseActions(),
            )
        }
        shots.snap("release_dead_archive") {
            ReleaseContent(
                release.copy(hit = F.releases[5], preview = Loadable.Ready(F.preview.copy(streamable = false)), details = null, notes = null),
                GrabUi.Idle,
                ReleaseActions(),
            )
        }
        shots.snap("release_owned") {
            ReleaseContent(release.copy(hit = F.releases[0], follow = FollowState.Following), GrabUi.Idle, ReleaseActions())
        }
        shots.snap("release_duplicate") {
            ReleaseContent(
                release.copy(hit = F.releases[3], follow = FollowState.Hidden),
                GrabUi.AskDuplicate("release", "Severance (2006) is already in your library."),
                ReleaseActions(),
            )
        }
    }
}
