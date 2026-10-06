package studio.kahn.iris.tv.ui.screens.player

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.theme.IrisTheme

/** The 1 s progress tick reaches the scrub bar only: the chrome around it does not recompose. */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = TV_QUALIFIERS)
class ScrubReadTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun aTickRecomposesTheScrubBarAlone() {
        var positionMs by mutableLongStateOf(0L)
        var chromeCompositions = 0
        compose.setContent {
            IrisTheme {
                chromeCompositions++
                val focus = remember { PlayerButtonFocus() }
                PlayerControls(
                    title = PlayerTitle("Severance", null, ""),
                    clock = null,
                    scrub = { ScrubPosition(positionMs = positionMs, bufferedMs = 0, durationMs = 3_600_000) },
                    buttons = PlayerButtons(playing = true, showTracks = false, sideLabel = null, nextLabel = null, trailing = ""),
                    focus = focus,
                    onPlayPause = {},
                    onTracks = {},
                    onSide = {},
                    onNext = {},
                    onLeaveButtons = {},
                )
            }
        }
        compose.waitForIdle()
        positionMs = 754_000L
        compose.waitForIdle()
        compose.onNodeWithText(clockText(754_000L)).assertExists()
        assertEquals(1, chromeCompositions)
    }
}
