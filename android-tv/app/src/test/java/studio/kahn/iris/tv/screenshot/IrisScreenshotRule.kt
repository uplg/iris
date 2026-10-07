package studio.kahn.iris.tv.screenshot

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.test.platform.app.InstrumentationRegistry
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.rules.RuleChain
import org.junit.rules.TestRule
import org.junit.runner.Description
import org.junit.runners.model.Statement
import studio.kahn.iris.tv.data.InterfaceSize
import studio.kahn.iris.tv.ui.theme.InterfaceScale
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisLayout
import studio.kahn.iris.tv.ui.theme.IrisTheme

/** The frames a layout must hold: the TV, and the landscape phones the same APK runs on. */
enum class ScreenSize(val id: String, val width: Dp, val height: Dp) {
    Tv("tv", 960.dp, 540.dp),
    PhoneLarge("phone915", 915.dp, 412.dp),
    PhoneSmall("phone800", 800.dp, 360.dp),
}

/** Robolectric qualifiers for the test activity: a 1080p TV (960x540 dp at xhdpi). */
const val TV_QUALIFIERS = "w960dp-h540dp-land-television-xhdpi"

/**
 * Renders composables in the Iris theme at a [ScreenSize] and records them
 * under `app/src/test/screenshots/<name>.png`.
 *
 * ```
 * @RunWith(RobolectricTestRunner::class)
 * @GraphicsMode(GraphicsMode.Mode.NATIVE)
 * @Config(qualifiers = TV_QUALIFIERS)
 * class HomeScreenshots {
 *     @get:Rule val shots = IrisScreenshotRule()
 *     @Test fun home() = shots.snapEverySize("home") { HomeContent(state = fakeHome) }
 * }
 * ```
 *
 * Record: `./gradlew :app:recordRoborazziDebug` (then open the PNGs).
 * Check against the committed ones: `./gradlew :app:verifyRoborazziDebug`.
 * Several snaps per test are fine: each swaps the content of the same
 * activity. Use [Modifier.focusOnStart] to show an element focused.
 */
class IrisScreenshotRule : TestRule {
    val compose = createComposeRule()

    private var frame by mutableStateOf<Frame?>(null)
    private var started = false

    private class Frame(val size: ScreenSize, val scale: InterfaceSize, val content: @Composable () -> Unit)

    override fun apply(base: Statement, description: Description): Statement =
        RuleChain.outerRule(compose).apply(base, description)

    /**
     * Records [content] filling a [size] frame on the ground color, drawn at the interface
     * [scale] (Settings → Interface size): the same pixels, that many fewer dp.
     */
    fun snap(name: String, size: ScreenSize = ScreenSize.Tv, scale: InterfaceSize = InterfaceSize.Default, content: @Composable () -> Unit) {
        if (!started) {
            started = true
            // D-pad mode: tv-material surfaces and focus rings behave as on a TV.
            InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
            compose.setContent {
                val current = frame ?: return@setContent
                val layout = IrisLayout(current.size.width / current.scale.scale, current.size.height / current.scale.scale)
                InterfaceScale(current.scale) {
                    IrisTheme(layoutOverride = layout) {
                        Box(
                            Modifier
                                .requiredSize(layout.width, layout.height)
                                .background(IrisColor.ground)
                                .testTag(ROOT),
                        ) {
                            // A new key per frame resets focus and remembered state between snaps.
                            androidx.compose.runtime.key(current) { current.content() }
                        }
                    }
                }
            }
            // Let the window settle (take focus) before the first frame, or
            // a focus request in that frame is lost.
            compose.waitForIdle()
        }
        frame = Frame(size, scale, content)
        compose.waitForIdle()
        compose.onNodeWithTag(ROOT).captureRoboImage("src/test/screenshots/$name.png")
    }

    /** [snap] at every [ScreenSize], named `<name>_tv`, `<name>_phone915`, `<name>_phone800`. */
    fun snapEverySize(name: String, content: @Composable () -> Unit) = snapEverySize(name, InterfaceSize.Default, content)

    /** [snapEverySize] at the interface [scale]. */
    fun snapEverySize(name: String, scale: InterfaceSize, content: @Composable () -> Unit) {
        ScreenSize.entries.forEach { size -> snap("${name}_${size.id}", size, scale, content) }
    }

    private companion object {
        const val ROOT = "iris-screenshot-root"
    }
}

/** Focuses this element once it is composed (screenshots of focused states). */
@Composable
fun Modifier.focusOnStart(): Modifier {
    val requester = remember { FocusRequester() }
    LaunchedEffect(Unit) { requester.requestFocus() }
    return focusRequester(requester)
}
