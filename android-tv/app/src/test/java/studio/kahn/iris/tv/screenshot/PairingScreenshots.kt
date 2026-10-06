package studio.kahn.iris.tv.screenshot

import androidx.compose.runtime.Composable
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import studio.kahn.iris.tv.ui.screens.PairingCode
import studio.kahn.iris.tv.ui.screens.PairingContent
import studio.kahn.iris.tv.ui.screens.PairingUiState
import studio.kahn.iris.tv.ui.state.Loadable
import studio.kahn.iris.tv.ui.state.UiError

/** The reference screen: its stateless body rendered from fake states, at every size. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = TV_QUALIFIERS)
class PairingScreenshots {
    @get:Rule
    val shots = IrisScreenshotRule()

    private val code = PairingCode("WX7K-ABCD", "https://iris.kahn.studio/account", "device", expiresAtMs = 0)

    @Composable
    private fun Pairing(state: PairingUiState) {
        PairingContent(
            state = state,
            onServerUrlChange = {},
            onGenerate = {},
            onCancel = {},
            onUsePassword = {},
        )
    }

    @Test
    fun form() = shots.snapEverySize("pairing_form") { Pairing(PairingUiState()) }

    @Test
    fun formFailed() = shots.snap("pairing_form_failed") {
        Pairing(PairingUiState(code = Loadable.Failed(UiError(UiError.OFFLINE_MESSAGE, code = UiError.NETWORK))))
    }

    @Test
    fun code() = shots.snapEverySize("pairing_code") { Pairing(PairingUiState(code = Loadable.Ready(code))) }

    @Test
    fun codeStale() = shots.snap("pairing_code_stale") {
        Pairing(PairingUiState(code = Loadable.Stale(code, UiError(UiError.OFFLINE_MESSAGE, code = UiError.NETWORK))))
    }
}
