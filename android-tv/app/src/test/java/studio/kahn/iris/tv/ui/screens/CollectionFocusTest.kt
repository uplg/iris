package studio.kahn.iris.tv.ui.screens

import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import studio.kahn.iris.tv.screenshot.LibraryFixtures as F
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.screens.library.CollectionUiState
import studio.kahn.iris.tv.ui.screens.library.collectionPage
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisTheme

/** A title's page opens on its banner's play button, a series' as a film's. */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = TV_QUALIFIERS)
class CollectionFocusTest {
    @get:Rule
    val compose = createComposeRule()

    private fun opens(state: CollectionUiState, label: String) {
        InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
        compose.setContent { IrisTheme { CollectionContent(state, CollectionActions()) } }
        compose.waitForIdle()
        compose.onNode(hasText(label) and hasClickAction(), useUnmergedTree = false).assertIsFocused()
    }

    @Test
    fun aSeriesOpensOnPlay() = opens(CollectionUiState(page = Loadable.Ready(collectionPage(F.series, null, emptyList(), 2L, F.now))), "Start S1:E1")

    @Test
    fun aFilmOpensOnPlay() = opens(CollectionUiState(page = Loadable.Ready(collectionPage(F.movie, null, emptyList(), null, F.now))), "Play")
}
