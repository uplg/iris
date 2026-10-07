package studio.kahn.iris.tv.ui.screens

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.requestFocus
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import studio.kahn.iris.tv.data.UpdateNotice
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.components.TopTab
import studio.kahn.iris.tv.ui.nav.TopLevelShell
import studio.kahn.iris.tv.ui.screens.home.CardAction
import studio.kahn.iris.tv.ui.screens.home.CardModel
import studio.kahn.iris.tv.ui.screens.home.HeroAction
import studio.kahn.iris.tv.ui.screens.home.HeroButton
import studio.kahn.iris.tv.ui.screens.home.HeroModel
import studio.kahn.iris.tv.ui.screens.home.HomeUiState
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisTheme

/**
 * The home's first focus goes to the hero's main action, with the hero in view, whatever
 * order the reads land in: the rows are composed (loading) before the hero is known.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = TV_QUALIFIERS)
class HomeFocusTest {
    @get:Rule
    val compose = createComposeRule()

    private val hero = HeroModel(
        key = "resume:c:1",
        eyebrow = "Continue where you left off",
        title = "Severance",
        meta = "Season 2 · Episode 4 · 23 min left",
        overview = "Mark and his team push deeper into Lumon.",
        art = null,
        actions = listOf(HeroButton(HeroAction.Resume, "Resume at 32:10"), HeroButton(HeroAction.AllEpisodes, "All episodes")),
    )

    private val cards = List(6) { i ->
        CardModel(
            key = "cw:c:$i",
            title = "Title $i",
            art = null,
            meta = "S1:E$i",
            progress = 0.5f,
            primary = CardAction.Play,
            menu = listOf(CardAction.Play),
        )
    }

    private var state by mutableStateOf(HomeUiState())
    private var update by mutableStateOf<UpdateNotice?>(null)

    private fun show() {
        InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
        compose.setContent {
            IrisTheme {
                TopLevelShell(TopTab.Home, "Leonard", onSelect = {}, onAccount = {}, headerOverContent = true) {
                    HomeContent(
                        state = state,
                        onHeroAction = {},
                        onCardAction = { _, _ -> },
                        onRetry = {},
                        onOpenDiscover = {},
                        onOpenLibrary = {},
                        onOpenSearch = {},
                        update = update,
                    )
                }
            }
        }
        compose.waitForIdle()
    }

    private fun arrive() {
        state = state.copy(hero = hero, heroPending = false, continueWatching = Loadable.Ready(cards))
        compose.waitForIdle()
    }

    private fun assertHeroFocusedAndShown() {
        compose.onNode(isFocused()).assert(hasText("Resume at 32:10"))
        compose.onNodeWithText("CONTINUE WHERE YOU LEFT OFF").assertIsDisplayed()
        compose.onNodeWithText("Severance").assertIsDisplayed()
    }

    @Test
    fun backUpToTheHeroShowsItWhole() {
        show()
        state = state.copy(
            hero = hero.copy(title = "May I Help You and the Sisters of the Long Night"),
            heroPending = false,
            continueWatching = Loadable.Ready(cards),
            watchlist = Loadable.Ready(cards.map { it.copy(key = "wl:${it.key}", title = "Followed ${it.title}") }),
            library = Loadable.Ready(cards.map { it.copy(key = "lib:${it.key}", title = "Owned ${it.title}") }),
        )
        compose.waitForIdle()
        compose.onNode(isFocused()).assert(hasText("Resume at 32:10"))
        repeat(3) {
            compose.onRoot().performKeyInput { pressKey(Key.DirectionDown) }
            compose.waitForIdle()
        }
        // The rows scrolled the hero out of view.
        assertEquals(0, compose.onAllNodesWithText("Resume at 32:10").fetchSemanticsNodes().size)
        repeat(3) {
            compose.onRoot().performKeyInput { pressKey(Key.DirectionUp) }
            compose.waitForIdle()
        }
        compose.onNode(isFocused()).assert(hasText("Resume at 32:10"))
        val title = compose.onNodeWithText("May I Help You and the Sisters of the Long Night").fetchSemanticsNode()
        val unclipped = compose.onNodeWithText("May I Help You and the Sisters of the Long Night").getUnclippedBoundsInRoot()
        assertEquals("the title is cut under the header", (unclipped.bottom - unclipped.top).value, with(compose.density) { title.boundsInRoot.height.toDp().value }, 0.5f)
        compose.onNodeWithText("CONTINUE WHERE YOU LEFT OFF").assertIsDisplayed()
    }

    @Test
    fun heroArrivingAfterTheRowsTakesTheFirstFocus() {
        show()
        arrive()
        assertHeroFocusedAndShown()
    }

    @Test
    fun aCardFocusedBeforeTheHeroCameIsNotACardLeft() {
        state = HomeUiState(continueWatching = Loadable.Ready(cards))
        show()
        compose.onNodeWithText("Title 0").requestFocus()
        compose.waitForIdle()
        compose.onNode(isFocused()).assert(hasText("Title 0"))
        arrive()
        assertHeroFocusedAndShown()
    }

    @Test
    fun heroArrivingUnderTheUpdateBannerTakesTheFirstFocus() {
        show()
        update = UpdateNotice("9.9.9", "1.6.0", null)
        compose.waitForIdle()
        arrive()
        assertHeroFocusedAndShown()
    }

    @Test
    fun updateBannerArrivingAfterTheHeroKeepsTheHeroFocusedAndShown() {
        show()
        arrive()
        update = UpdateNotice("9.9.9", "1.6.0", null)
        compose.waitForIdle()
        assertHeroFocusedAndShown()
    }
}
