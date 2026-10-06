package studio.kahn.iris.tv.screenshot

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Text
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.ui.components.TopTab
import studio.kahn.iris.tv.ui.nav.ClientOutdatedOverlay
import studio.kahn.iris.tv.ui.nav.TopLevelShell
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisShape
import studio.kahn.iris.tv.ui.theme.IrisType

/** The shell around a section (the area its content gets, framed) and the update lock. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class ShellScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    @Test
    fun section() = shots.snapEverySize("shell_library") {
        TopLevelShell(TopTab.Library, "Leonard", onSelect = {}, onAccount = {}) {
            val layout = IrisLayout.current
            Box(
                Modifier
                    .fillMaxSize()
                    .padding(start = layout.safeHorizontal, end = layout.safeHorizontal, bottom = layout.safeVertical)
                    .border(1.dp, IrisColor.line, IrisShape.card),
                contentAlignment = Alignment.Center,
            ) {
                Text("The section's content", style = IrisType.meta, color = IrisColor.inkMuted)
            }
        }
    }

    @Test
    fun clientOutdated() = shots.snapEverySize("shell_client_outdated") {
        ClientOutdatedOverlay(installedVersion = "1.5.0", onOpenSettings = {})
    }
}
