package studio.kahn.iris.tv.ui.theme

import androidx.compose.ui.graphics.Color

/**
 * The synthe.se palette, dark side, as `web/src/styles/tokens.css` declares it
 * (names follow the web custom properties, camel-cased). Ground, ink and ONE
 * hue, petrol; warn and down for states, always said in words beside them.
 * No gradients, no glow, no glass: every fill below is flat.
 *
 * The only colors allowed outside `ui/theme/`. The player is a dark "stage"
 * of its own ([stage] and friends), the same in every theme.
 */
object IrisColor {
    val ground = Color(0xFF151A21)
    val groundRaised = Color(0xFF1E2530)
    val surface = Color(0xFF1A2029)
    val line = Color(0xFF2E3744)
    val ink = Color(0xFFEAE7E0)
    val inkMuted = Color(0xFFA8AEB8)
    /** Muted text on an [ink] fill (a focused row's secondary line). */
    val inkMutedOnInk = Color(0xFF525A66)

    val accent = Color(0xFF63C0CB)
    /** UI and large text only (3.6:1 on ground). */
    val accentSoft = Color(0xFF3E7A84)
    val accentWash = Color(0xFF1D2A31)
    val onAccent = Color(0xFF151A21)

    /** web `--status-degraded` / `--warn-text`. */
    val warn = Color(0xFFD89372)
    /** web `--status-down` / `--blocked`. */
    val down = Color(0xFFE07A63)
    val downWash = Color(0xFF33211F)

    /** Behind a dialog. */
    val overlay = Color(0xB8151A21)

    /** The fallback tile behind a poster or still with no artwork, and its title. */
    val art = Color(0xFF24323F)
    val artInk = Color(0xFFF2F0EA)
    /** A provider tag laid over artwork. */
    val artScrim = Color(0xC70E1218)

    val stage = Color(0xFF0E1218)
    val stageRaised = Color(0xFF1F2B36)
    val stageInk = Color(0xFFEAE7E0)
    val stageMuted = Color(0xFFA8AEB8)
    /** The player's top and bottom bars (web `--stage-scrim`). */
    val stageScrim = Color(0xDB0E1218)
    /** The seek track, and a key cap's frame on the stage. */
    val stageLine = Color(0x2EFFFFFF)
    /** The buffered part of the seek track. */
    val stageFill = Color(0x52FFFFFF)

    val cueBackground = Color(0xC7000000)
    val cueInk = Color(0xFFFFFFFF)
}
