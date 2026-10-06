package studio.kahn.iris.tv.screenshot

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import java.time.ZoneOffset
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.data.PlaybackPrefsResponse
import studio.kahn.iris.tv.ui.screens.CollectionContent
import studio.kahn.iris.tv.ui.screens.DetailContent
import studio.kahn.iris.tv.ui.screens.EpisodeSheet
import studio.kahn.iris.tv.ui.screens.FindAndSortPanel
import studio.kahn.iris.tv.ui.screens.HistoryActions
import studio.kahn.iris.tv.ui.screens.HistoryContent
import studio.kahn.iris.tv.ui.screens.LanguagesPanel
import studio.kahn.iris.tv.ui.screens.LibraryActions
import studio.kahn.iris.tv.ui.screens.LibraryContent
import studio.kahn.iris.tv.ui.nav.TopLevelShell
import studio.kahn.iris.tv.ui.components.TopTab
import androidx.compose.runtime.Composable
import studio.kahn.iris.tv.ui.screens.CollectionActions
import studio.kahn.iris.tv.ui.screens.DetailActions
import studio.kahn.iris.tv.ui.screens.library.CollectionUiState
import studio.kahn.iris.tv.ui.screens.library.DetailUiState
import studio.kahn.iris.tv.ui.screens.library.HistoryUiState
import studio.kahn.iris.tv.ui.screens.library.LibraryUiState
import studio.kahn.iris.tv.ui.screens.library.LibraryView
import studio.kahn.iris.tv.ui.screens.library.ReleaseActions
import studio.kahn.iris.tv.ui.screens.library.ReleasePage
import studio.kahn.iris.tv.ui.screens.library.ReleaseFile
import studio.kahn.iris.tv.ui.screens.library.TitleFilters
import studio.kahn.iris.tv.ui.screens.library.Torrents
import studio.kahn.iris.tv.ui.screens.library.collectionPage
import studio.kahn.iris.tv.ui.screens.library.downloadsUi
import studio.kahn.iris.tv.ui.screens.library.historyUi
import studio.kahn.iris.tv.ui.screens.library.languagesUi
import studio.kahn.iris.tv.ui.screens.library.libraryFacts
import studio.kahn.iris.tv.ui.screens.library.releaseRow
import studio.kahn.iris.tv.ui.screens.library.titleCounts
import studio.kahn.iris.tv.ui.screens.library.titlesUi
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError
import studio.kahn.iris.tv.screenshot.LibraryFixtures as F
import studio.kahn.iris.tv.ui.components.Notice

/** The library, a title's page, a release and the history, from fake server answers. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class LibraryScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    @Composable
    private fun ShelledLibrary(state: LibraryUiState, actions: LibraryActions) {
        TopLevelShell(TopTab.Library, accountName = "Leonard", onSelect = {}, onAccount = {}) {
            LibraryContent(state, actions)
        }
    }

    private val releaseActions = ReleaseActions(onPlay = { _, _ -> }, onFiles = {}, onOpenTitle = {})

    private fun library(view: LibraryView = LibraryView.Titles, filters: TitleFilters = TitleFilters()) = LibraryUiState(
        view = view,
        facts = libraryFacts(titleCounts(F.titles), F.summary),
        filters = filters,
        titles = Loadable.Ready(titlesUi(F.titles, F.torrents, F.watching, emptyList(), filters)),
        downloads = Loadable.Ready(downloadsUi(Torrents(F.torrents, 120L shl 30, 96L shl 30), F.titles, F.watching, "", F.now)),
    )

    @Test
    fun titles() = shots.snapEverySize("library_titles") { ShelledLibrary(library(), LibraryActions(onRelease = releaseActions)) }

    @Test
    fun titlesFiltered() = shots.snap("library_titles_series") {
        ShelledLibrary(library(filters = TitleFilters(type = studio.kahn.iris.tv.ui.screens.library.TypeFilter.Series)), LibraryActions(onRelease = releaseActions))
    }

    @Test
    fun titlesLoading() = shots.snap("library_titles_loading") {
        ShelledLibrary(LibraryUiState(), LibraryActions(onRelease = releaseActions))
    }

    @Test
    fun titlesEmpty() = shots.snap("library_titles_empty") {
        ShelledLibrary(LibraryUiState(titles = Loadable.Ready(titlesUi(emptyList(), emptyList(), emptyList(), emptyList(), TitleFilters()))), LibraryActions(onRelease = releaseActions))
    }

    @Test
    fun titlesFailed() = shots.snap("library_titles_failed") {
        ShelledLibrary(LibraryUiState(titles = Loadable.Failed(UiError(UiError.OFFLINE_MESSAGE, UiError.NETWORK))), LibraryActions(onRelease = releaseActions))
    }

    @Test
    fun findAndSort() = shots.snap("library_find_sort") {
        Box(Modifier.fillMaxSize()) {
            ShelledLibrary(library(), LibraryActions(onRelease = releaseActions))
            FindAndSortPanel(library(), focusSort = true, onFilters = {}, onDismiss = {})
        }
    }

    @Test
    fun downloads() = shots.snapEverySize("library_downloads") {
        ShelledLibrary(
            library(LibraryView.Downloads).copy(notice = Notice("Paused Perfect Days. Its files stay on disk.", failed = false)),
            LibraryActions(onRelease = releaseActions),
        )
    }

    private fun series() = CollectionUiState(
        page = Loadable.Ready(collectionPage(F.series, null, emptyList(), emptyMap(), 2L, F.now)),
        languages = Loadable.Ready(languagesUi(PlaybackPrefsResponse(audioLanguage = "en", subtitleLanguage = "off", forCollection = true), F.series, null)),
    )

    @Test
    fun collectionSeries() = shots.snapEverySize("collection_series") { CollectionContent(series(), CollectionActions(onRelease = releaseActions.copy(onOpenTitle = null))) }

    @Test
    fun collectionMovie() = shots.snap("collection_movie") {
        CollectionContent(CollectionUiState(page = Loadable.Ready(collectionPage(F.movie, null, emptyList(), emptyMap(), null, F.now))), CollectionActions(onRelease = releaseActions.copy(onOpenTitle = null)))
    }

    @Test
    fun collectionLoading() = shots.snap("collection_loading") { CollectionContent(CollectionUiState(), CollectionActions(onRelease = releaseActions.copy(onOpenTitle = null))) }

    @Test
    fun episodeSheet() = shots.snap("collection_episode_sheet") {
        val page = series().page.valueOrNull!!
        Box(Modifier.fillMaxSize()) {
            CollectionContent(series(), CollectionActions(onRelease = releaseActions.copy(onOpenTitle = null)))
            EpisodeSheet(page.episodes.first { it.key == "2-7" }, emptySet(), { _, _ -> }, {})
        }
    }

    @Test
    fun languages() = shots.snap("collection_languages") {
        Box(Modifier.fillMaxSize()) {
            CollectionContent(series(), CollectionActions(onRelease = releaseActions.copy(onOpenTitle = null)))
            LanguagesPanel("Severance", series().languages!!.valueOrNull!!, saving = false, onSave = { _, _ -> }, onDismiss = {})
        }
    }

    @Test
    fun release() = shots.snapEverySize("release_detail") {
        val t = F.torrents[3]
        DetailContent(
            DetailUiState(
                page = Loadable.Ready(
                    ReleasePage(
                        title = "Severance",
                        kind = "Series",
                        posterUrl = null,
                        row = releaseRow(t, "Severance", null, null, F.now).copy(playIdx = null),
                        files = t.files.mapIndexed { i, f ->
                            ReleaseFile(f.index, f.path, if (i < 3) "2.0 GB · Watched" else "2.0 GB", resume = i == 3, watched = i < 3)
                        },
                    ),
                ),
            ),
            DetailActions(),
        )
    }

    @Test
    fun history() = shots.snapEverySize("history") {
        HistoryContent(HistoryUiState(groups = Loadable.Ready(historyUi(F.history, F.now, ZoneOffset.UTC))), HistoryActions())
    }

    @Test
    fun historyEmpty() = shots.snap("history_empty") {
        HistoryContent(HistoryUiState(groups = Loadable.Ready(emptyList())), HistoryActions())
    }
}
