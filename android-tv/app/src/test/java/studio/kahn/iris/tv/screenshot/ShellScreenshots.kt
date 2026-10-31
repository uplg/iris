package studio.kahn.iris.tv.screenshot

import studio.kahn.iris.tv.ui.screens.live.toneOf
import studio.kahn.iris.tv.ui.nav.ShellBackdrop
import studio.kahn.iris.tv.ui.nav.LocalShellBackdrop
import androidx.compose.ui.graphics.Color
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Composable
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.background
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
import studio.kahn.iris.tv.data.AppUpdater
import studio.kahn.iris.tv.data.UpdateState
import studio.kahn.iris.tv.ui.update.UpdateActions
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
        ClientOutdatedOverlay(outdated, UpdateActions(), onOpenSettings = {})
    }

    @Test
    fun clientOutdatedDownloading() = shots.snap("shell_client_outdated_downloading") {
        ClientOutdatedOverlay(outdated.copy(progress = AppUpdater.Progress.Downloading(31_000_000, 52_000_000)), UpdateActions(), onOpenSettings = {})
    }

    @Test
    fun headerUpdate() = shots.snapEverySize("shell_header_update") {
        TopLevelShell(TopTab.Library, "Leonard", updateAvailable = true, onSelect = {}, onAccount = {}) {}
    }

    @Composable
    private fun OverStill(still: Color, ink: Color?) {
        TopLevelShell(TopTab.Home, "Uplg", onSelect = {}, onAccount = {}, headerOverContent = true) {
            ShellBackdrop { Box(Modifier.size(1280.dp, 720.dp).background(still)) }
            val holder = LocalShellBackdrop.current
            SideEffect { holder?.ink = ink }
        }
    }

    @Test
    fun nameBeforeOnALightStill() = shots.snap("shell_name_over_light_before") { OverStill(LIGHT_STILL, null) }

    @Test
    fun nameOnALightStill() = shots.snap("shell_name_over_light") { OverStill(LIGHT_STILL, toneOf(0.8).plate) }

    @Test
    fun nameOnADarkStill() = shots.snap("shell_name_over_dark") { OverStill(DARK_STILL, toneOf(0.02).plate) }

    private val outdated = UpdateState(installed = "1.5.0", latest = AppUpdater.VersionStatus.UpdateAvailable("1.6.0"))
}

private val LIGHT_STILL = Color(0xFFF2E8D5)
private val DARK_STILL = Color(0xFF101418)
