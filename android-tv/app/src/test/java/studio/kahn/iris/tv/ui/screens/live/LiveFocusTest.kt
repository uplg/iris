package studio.kahn.iris.tv.ui.screens.live

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.onNodeWithText
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import studio.kahn.iris.tv.data.LiveChannel
import studio.kahn.iris.tv.data.LiveCountry
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.theme.IrisTheme

/** Focus on the channel list comes back to the channel left, and to the new country's first channel. */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = TV_QUALIFIERS)
class LiveFocusTest {
    @get:Rule
    val compose = createComposeRule()

    private fun channels(prefix: String) = List(24) { i ->
        LiveChannel(categories = emptyList(), geoBlocked = false, id = "$prefix$i", name = "$prefix channel ${i + 1}", not247 = false)
    }

    private val countries = listOf(
        LiveCountry(code = "fr", flag = "FR", name = "France", channelCount = 24),
        LiveCountry(code = "be", flag = "BE", name = "Belgium", channelCount = 24),
    )

    private var shown by mutableStateOf(true)
    private var country by mutableStateOf("fr")

    private fun show() {
        InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
        compose.setContent {
            IrisTheme {
                val holder = rememberSaveableStateHolder()
                if (shown) {
                    holder.SaveableStateProvider("live") {
                        LiveTvContent(
                            ui = LiveTvUi(
                                countries = countries,
                                country = country,
                                channels = Loadable.Ready(channels(if (country == "fr") "France" else "Belgium")),
                                guide = LiveGuide(),
                                query = "",
                                results = null,
                            ),
                            clock = { "" },
                            countryName = { it },
                            onQueryChange = {},
                            onPickCountry = { country = it },
                            onOpen = { _, _ -> shown = false },
                            onRetry = {},
                        )
                    }
                }
            }
        }
        compose.waitForIdle()
    }

    private fun focusedText(): String? =
        compose.onNode(isFocused()).fetchSemanticsNode().config.getOrNull(SemanticsProperties.Text)?.joinToString()

    @Test
    fun backFromAChannelFocusesItAgain() {
        show()
        compose.onNode(isFocused()).assert(hasText("France channel 1"))
        repeat(3) {
            compose.onRoot().performKeyInput { pressKey(Key.DirectionDown) }
            compose.waitForIdle()
        }
        val left = focusedText()
        assertNotEquals("France channel 1", left)
        compose.onRoot().performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitForIdle()
        assertEquals(false, shown)
        shown = true
        compose.waitForIdle()
        assertEquals(left, focusedText())
    }

    @Test
    fun pickingACountryFocusesItsFirstChannel() {
        show()
        compose.onNodeWithText("FR France").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("BE Belgium · 24 channels").performClick()
        compose.waitForIdle()
        compose.onNode(isFocused()).assert(hasText("Belgium channel 1"))
    }
}
