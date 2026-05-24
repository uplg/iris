package studio.kahn.iris.tv.ui.theme

import androidx.compose.runtime.Composable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.Dp
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
    val xxxl = 48.dp
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
    val xl = 16.dp
    /** Poster / large card corner (web `.poster` 18px, `.detail-poster` 24px). */
    val poster = 18.dp
    /** Buttons (web `.btn` 14px). */
    val button = 14.dp
    /** Glass panels / search bar (web `.search-bar` 22px). */
    val panel = 20.dp
    /** Pill / fully-rounded (chips, progress bars). */
    val pill = 999.dp
}

/**
 * Focus treatment constants shared by the focusable design-system
 * components, so the brand ring + lift + glow read identically whether the
 * focused thing is a poster, a button, or an icon chip (web: `box-shadow:
 * 0 0 0 3px brand, 0 0 0 7px brand-soft` + `translateY` + glow).
 */
object Focus {
    /** Brand ring stroke width on focus. */
    val ring = 3.dp
    /** Poster pop on focus (web `.card[data-focused] scale(1.06)`). */
    const val posterScale = 1.06f
    /** Button / chip pop (web `.btn` `scale(1.02)`). */
    const val controlScale = 1.04f
    /** Brand glow elevation behind a focused surface. */
    val glow = 16.dp
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

/**
 * Density bucket of the current TV. The Compose-on-TV ecosystem doesn't
 * ship a `WindowSizeClass` analogue — we derive it from the Configuration's
 * `smallestScreenWidthDp` and use it to tune card sizes, gutters and
 * column counts so the layout stays comfortable on a 1280×720 Mi Box,
 * a 1080p panel, and a 3840×2160 Sony Bravia alike.
 *
 * Buckets in `dp` (not pixels — Compose has already applied `densityDpi`):
 *   * **Compact** ≤ 720 dp wide. 720p panels, older Android TV boxes.
 *   * **Medium**  ≤ 1080 dp wide. The mainstream 1080p TV.
 *   * **Expanded** > 1080 dp.    1440p+ and 4K.
 */
enum class TvSizeClass { Compact, Medium, Expanded }

@Composable
@ReadOnlyComposable
fun rememberTvSizeClass(): TvSizeClass {
    val widthDp = LocalConfiguration.current.screenWidthDp
    return when {
        widthDp <= 720 -> TvSizeClass.Compact
        widthDp <= 1080 -> TvSizeClass.Medium
        else -> TvSizeClass.Expanded
    }
}

/**
 * Layout constants picked per [TvSizeClass]. Pulled into a single struct
 * so `gutter`, `posterMin`, `shelfPosterWidth` move together — without
 * this, "make the search grid wider" inevitably forgot to also widen
 * the home shelves.
 */
data class TvLayout(
    val gutterHorizontal: Dp,
    val gutterVertical: Dp,
    /** Minimum width for `LazyVerticalGrid(GridCells.Adaptive)`. */
    val gridPosterMin: Dp,
    /** Fixed width for poster cards on horizontal shelves. */
    val shelfPosterWidth: Dp,
    /** Hero (backdrop) aspect ratio. 16:5 on tight panels (Mi Box) so
     *  the poster + title still fit above the fold; 16:7 on roomier
     *  panels for the full Netflix-style proportions. */
    val heroAspect: Float,
    /** Side rail width on the multi-column detail screens. */
    val detailRail: Dp,
)

private val Compact = TvLayout(
    gutterHorizontal = 24.dp,
    gutterVertical = 20.dp,
    gridPosterMin = 140.dp,
    shelfPosterWidth = 124.dp,
    heroAspect = 16f / 5f,
    detailRail = 220.dp,
)

private val Medium = TvLayout(
    gutterHorizontal = 32.dp,
    gutterVertical = 24.dp,
    gridPosterMin = 160.dp,
    shelfPosterWidth = 140.dp,
    heroAspect = 16f / 6f,
    detailRail = 280.dp,
)

private val Expanded = TvLayout(
    gutterHorizontal = 48.dp,
    gutterVertical = 32.dp,
    gridPosterMin = 200.dp,
    shelfPosterWidth = 180.dp,
    heroAspect = 16f / 7f,
    detailRail = 360.dp,
)

@Composable
@ReadOnlyComposable
fun rememberTvLayout(): TvLayout = when (rememberTvSizeClass()) {
    TvSizeClass.Compact -> Compact
    TvSizeClass.Medium -> Medium
    TvSizeClass.Expanded -> Expanded
}

/** Used by composables that need to look up the active layout outside
 *  a `rememberTvLayout()` call site (rare but handy for e.g. one-off
 *  computations inside a child composable). */
val LocalTvLayout = staticCompositionLocalOf { Medium }
