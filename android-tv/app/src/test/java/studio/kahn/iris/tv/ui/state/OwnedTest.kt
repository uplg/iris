package studio.kahn.iris.tv.ui.state

import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.v2.createComposeRule
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** A player the composition owns is released once, after the effects that still read it. */
@RunWith(RobolectricTestRunner::class)
class OwnedTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun releasedAfterTheEffectsThatUseItAndOnAKeyChange() {
        val log = mutableListOf<String>()
        var key by mutableStateOf(1)
        var shown by mutableStateOf(true)
        compose.setContent {
            if (shown) {
                val k = key
                val player = remember(k) { Owned("player $k") { log += "release $it" } }.value
                DisposableEffect(player) { onDispose { log += "last save on $player" } }
            }
        }
        compose.waitForIdle()
        key = 2
        compose.waitForIdle()
        assertEquals(listOf("last save on player 1", "release player 1"), log)
        shown = false
        compose.waitForIdle()
        assertEquals(listOf("last save on player 1", "release player 1", "last save on player 2", "release player 2"), log)
    }
}
