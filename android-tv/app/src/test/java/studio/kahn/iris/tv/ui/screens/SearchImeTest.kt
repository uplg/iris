package studio.kahn.iris.tv.ui.screens

import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.InterceptPlatformTextInput
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.awaitCancellation
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.screens.search.GrabUi
import studio.kahn.iris.tv.ui.screens.search.SearchUiState
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisTheme

/**
 * With the on-screen keyboard the search field never asks the platform for a text input
 * session (a TV keyboard would open over the on-screen one); without it, it does.
 */
@OptIn(ExperimentalComposeUiApi::class, ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = TV_QUALIFIERS)
class SearchImeTest {
    @get:Rule
    val compose = createComposeRule()

    private var sessions = 0
    private val typed = mutableListOf<String>()

    private fun show(onScreenKeyboard: Boolean) {
        InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
        compose.setContent {
            IrisTheme {
                // Stands for the platform: every session the field asks for reaches it.
                InterceptPlatformTextInput(interceptor = { _, _ ->
                    sessions++
                    awaitCancellation()
                }) {
                    SearchContent(
                        state = SearchUiState(recent = Loadable.Ready(emptyList())),
                        grab = GrabUi.Idle,
                        onScreenKeyboard = onScreenKeyboard,
                        actions = SearchActions(onType = { typed += it }),
                    )
                }
            }
        }
        compose.waitForIdle()
    }

    private fun pressOkOnTheField() {
        val field = compose.onNode(isFocused())
        field.assert(hasContentDescription("Title, year or release name"))
        field.performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitForIdle()
    }

    @Test
    fun withTheOnScreenKeyboardTheFieldNeverStartsAnInputSession() {
        show(onScreenKeyboard = true)
        pressOkOnTheField()
        assertEquals(0, sessions)
    }

    @Test
    fun withTheOnScreenKeyboardAHardwareKeyboardStillTypes() {
        show(onScreenKeyboard = true)
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.A) }
        compose.waitForIdle()
        assertEquals(listOf("a"), typed)
    }

    @Test
    fun withoutTheOnScreenKeyboardTheFieldStartsAnInputSession() {
        show(onScreenKeyboard = false)
        pressOkOnTheField()
        assertEquals(true, sessions > 0)
    }
}
