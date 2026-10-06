package studio.kahn.iris.tv.ui.components

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.theme.IrisTheme

/** The shared stage card and dialog shell behave the same wherever they show. */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = TV_QUALIFIERS)
class SharedPartsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun show(content: @androidx.compose.runtime.Composable () -> Unit) {
        InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
        compose.setContent { IrisTheme { content() } }
        compose.waitForIdle()
    }

    @Test
    fun theStageCardFocusesTryAgainWhenItCanHelp() {
        show { StageErrorCard("The player stopped", "Network read failed.", onRetry = {}, backLabel = "Back", onBack = {}) }
        compose.onNode(isFocused()).assert(hasText("Try again"))
    }

    @Test
    fun theStageCardFocusesTheWayOutWhenNothingCanHelp() {
        show { StageErrorCard("This channel can't be played", "Encrypted.", onRetry = null, backLabel = "Back to channels", onBack = {}) }
        compose.onNode(isFocused()).assert(hasText("Back to channels"))
        compose.onNodeWithText("Try again").assertDoesNotExist()
    }

    @Test
    fun backCancelsADialog() {
        var cancelled = 0
        show { ConfirmDialog("Delete a release", "Delete it?", "Delete release", onConfirm = {}, onCancel = { cancelled++ }) }
        compose.onNode(isFocused()).assert(hasText("Delete release"))
        compose.runOnIdle { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        assertEquals(1, cancelled)
    }
}
