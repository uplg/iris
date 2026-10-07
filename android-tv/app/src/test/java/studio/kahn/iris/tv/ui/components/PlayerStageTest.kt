package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import android.view.View
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.core.view.drawToBitmap
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.screenshot.TV_QUALIFIERS
import studio.kahn.iris.tv.ui.theme.IrisTheme

/** What frames the picture is black up to the screen's edges: no blue-black fringe above or below a film. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class PlayerStageTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun theEdgesAroundThePictureAreBlack() {
        var view: View? = null
        compose.setContent {
            view = LocalView.current
            IrisTheme { Box(Modifier.fillMaxSize()) { PlayerStage(player = null) } }
        }
        compose.waitForIdle()
        val pixels = view!!.drawToBitmap()
        val black = Color.Black.toArgb()
        for (y in listOf(0, 1, pixels.height / 2, pixels.height - 2, pixels.height - 1)) {
            for (x in listOf(0, pixels.width / 2, pixels.width - 1)) {
                assertEquals("pixel at $x,$y", black, pixels.getPixel(x, y))
            }
        }
    }
}
