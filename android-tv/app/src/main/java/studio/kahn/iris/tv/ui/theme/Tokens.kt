package studio.kahn.iris.tv.ui.theme

import androidx.compose.ui.unit.dp

/**
 * Centralised spacing scale. Use these instead of literal `Modifier.padding(16.dp)`
 * calls so the visual rhythm of every screen comes from one place.
 *
 * Scale rationale:
 *   * 4 dp grid (`xs`, `sm`, `md`, `lg`, `xl`, `xxl`) — Material's default
 *     baseline. Doubles cleanly which suits TV's bigger UI footprint.
 *   * `gutter` (40 dp) is the standard outer page margin — matches the
 *     hero / shelf indents on HomeScreen.
 */
object Spacing {
    val xs = 4.dp
    val sm = 8.dp
    val md = 12.dp
    val lg = 16.dp
    val xl = 24.dp
    val xxl = 32.dp
    val gutter = 40.dp
}

/**
 * Corner radius scale used by Card / Surface / Button shapes. Picks from
 * the same handful of values across the app so focus highlights land on
 * a consistent silhouette.
 */
object Radius {
    val sm = 4.dp
    val md = 8.dp
    val lg = 12.dp
}

/**
 * Card width scale for poster-style cards. `Sm` is the Watchlist /
 * Continue Watching size; `Md` for hero-adjacent featured cards.
 */
object CardSize {
    val sm = 140.dp
    val md = 180.dp
    val lg = 220.dp
}
