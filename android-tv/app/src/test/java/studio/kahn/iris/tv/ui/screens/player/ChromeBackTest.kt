package studio.kahn.iris.tv.ui.screens.player

import android.app.Activity
import android.view.KeyEvent
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.theme.IrisTheme

/**
 * Back hides the player's chrome in one press, the focus in its buttons or on the picture.
 * The keys go through the activity as a remote's do: unhandled, a Back press with the focus
 * in the buttons only took the focus out of them, and the chrome stayed for a second press.
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = TV_QUALIFIERS)
class ChromeBackTest {
    @get:Rule
    val compose = createComposeRule()

    private lateinit var activity: Activity
    private var screenBacks = 0

    private fun show(chrome: ChromeState) {
        InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
        compose.setContent {
            activity = LocalActivity.current!!
            // The screen's own Back (leaving the player): registered first, so the chrome's wins.
            BackHandler { screenBacks++ }
            IrisTheme {
                val root = remember { FocusRequester() }
                val buttons = remember { PlayerButtonFocus() }
                Box(
                    Modifier
                        .fillMaxSize()
                        .hideOnBack(chrome) { root.requestFocus() }
                        .focusRequester(root)
                        .focusable(),
                ) {
                    if (chrome.mode != ChromeMode.Hidden) {
                        PlayerBottomBar(
                            scrub = { ScrubPosition(positionMs = 60_000, bufferedMs = 90_000, durationMs = 3_000_000) },
                            buttons = PlayerButtons(playing = true, showTracks = true, sideLabel = "Episodes", nextLabel = null, trailing = ""),
                            focus = buttons,
                            onPlayPause = {},
                            onTracks = {},
                            onSide = {},
                            onNext = {},
                            onLeaveButtons = {},
                            modifier = Modifier.align(Alignment.BottomStart),
                        )
                    }
                }
                LaunchedEffect(Unit) {
                    if (chrome.mode == ChromeMode.Engaged) buttons.play.requestFocus() else root.requestFocus()
                }
            }
        }
        compose.waitForIdle()
    }

    private fun pressBack() {
        compose.runOnUiThread {
            activity.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_BACK))
            activity.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_BACK))
        }
        compose.waitForIdle()
    }

    @Test
    fun oneBackHidesTheButtons() {
        val chrome = ChromeState().apply { mode = ChromeMode.Engaged }
        show(chrome)
        pressBack()
        assertEquals(ChromeMode.Hidden, chrome.mode)
        assertEquals(0, screenBacks)
    }

    @Test
    fun oneBackHidesTheBarOverThePicture() {
        val chrome = ChromeState().apply { mode = ChromeMode.Peek }
        show(chrome)
        pressBack()
        assertEquals(ChromeMode.Hidden, chrome.mode)
        assertEquals(0, screenBacks)
    }

    @Test
    fun withTheChromeHiddenBackLeaves() {
        val chrome = ChromeState()
        show(chrome)
        pressBack()
        assertEquals(1, screenBacks)
    }
}
