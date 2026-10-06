package studio.kahn.iris.tv.ui.nav

import androidx.activity.ComponentActivity
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.components.ActionButton
import studio.kahn.iris.tv.ui.components.TopTab
import studio.kahn.iris.tv.ui.theme.IrisTheme

/** Back on a section: content → its tab in the header → Home → out of the app. */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = TV_QUALIFIERS)
class ShellBackTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private var selected: TopTab? = null

    private fun shell(tab: TopTab) {
        InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
        compose.setContent {
            IrisTheme {
                TopLevelShell(tab, "Leonard", onSelect = { selected = it }, onAccount = {}) {
                    val inside = remember { FocusRequester() }
                    LaunchedEffect(Unit) { inside.requestFocus() }
                    ActionButton("Inside", {}, modifier = Modifier.focusRequester(inside))
                }
            }
        }
        compose.waitForIdle()
        compose.onNode(isFocused()).assert(hasText("Inside"))
    }

    private fun back() {
        compose.runOnIdle { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
    }

    @Test
    fun aSectionGoesBackToItsTabThenHome() {
        shell(TopTab.Library)
        back()
        compose.onNode(isFocused()).assert(hasText("Library"))
        assertNull(selected)
        back()
        assertEquals(TopTab.Home, selected)
    }

    @Test
    fun homeLetsThePlatformLeave() {
        shell(TopTab.Home)
        back()
        compose.onNode(isFocused()).assert(hasText("Home"))
        assertEquals(false, compose.activity.onBackPressedDispatcher.hasEnabledCallbacks())
    }
}
