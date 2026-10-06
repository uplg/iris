package studio.kahn.iris.tv.ui.theme

import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import androidx.tv.material3.Typography as TvTypography
import studio.kahn.iris.tv.R

/*
 * Fraunces (web `--font-display`), one static cut per optical size: Compose can't drive the
 * variable `opsz` axis below API 26, and the web lets the browser pick it from the size (a
 * board px is 1/2 sp). The high-contrast 72 for the hero, the sturdier 48 for page and
 * title names. Fraunces sets short titles at 20 sp and up, nothing else: below 20 sp a serif
 * reads poorly from the couch, and a sentence or figures (a year, a season, an infohash, a
 * dialog title) read better in Cal Sans 2 at any size: see [heading] and [IrisType.titleFor].
 */
private val FrauncesDisplay = FontFamily(Font(R.font.fraunces_opsz72_medium, FontWeight.Medium))
private val FrauncesTitle = FontFamily(Font(R.font.fraunces_opsz48_medium, FontWeight.Medium))

/** Cal Sans 2: every text that is not a title (web `--font-text`). */
val CalSans = FontFamily(
    Font(R.font.cal_sans_regular, FontWeight.Normal),
    Font(R.font.cal_sans_medium, FontWeight.Medium),
    Font(R.font.cal_sans_semibold, FontWeight.SemiBold),
    Font(R.font.cal_sans_bold, FontWeight.Bold),
)

/** Borel: the "Iris" wordmark only (web `--font-brand`). */
val Borel = FontFamily(Font(R.font.borel_display_regular, FontWeight.Normal))

/** Release names and file names (web `--font-mono`: the system monospace). */
val FontMono: FontFamily = FontFamily.Monospace

private const val TABULAR = "tnum"

private val tight = LineHeightStyle(
    alignment = LineHeightStyle.Alignment.Center,
    trim = LineHeightStyle.Trim.None,
)

private fun display(size: TextUnit, line: TextUnit) = TextStyle(
    fontFamily = if (size.value >= 36f) FrauncesDisplay else FrauncesTitle,
    fontWeight = FontWeight.Medium,
    fontSize = size,
    lineHeight = line,
    letterSpacing = (-0.01).em,
    lineHeightStyle = tight,
)

private fun text(
    size: TextUnit,
    line: TextUnit,
    weight: FontWeight = FontWeight.Normal,
    tabular: Boolean = false,
) = TextStyle(
    fontFamily = CalSans,
    fontWeight = weight,
    fontSize = size,
    lineHeight = line,
    fontFeatureSettings = if (tabular) TABULAR else null,
    lineHeightStyle = tight,
)

private fun heading(size: TextUnit, line: TextUnit) = text(size, line, FontWeight.SemiBold)

/**
 * The type scale of the TV boards. Boards are 1920x1080 px = 960x540 dp, so
 * every size here is the board's px / 2 (named after where the board uses it).
 * Times, sizes and counts use [meta] / [metaLarge] / [figure], which carry
 * tabular figures so columns of numbers line up and a ticking clock does
 * not jitter.
 */
object IrisType {
    /** 84 px: the home hero title. */
    val hero = display(42.sp, 43.sp)
    /** 56 px: the getting-ready title. */
    val headline = display(28.sp, 30.sp)
    /** 52 px: a title or release page heading. */
    val title = display(26.sp, 28.sp)
    /** 48 px: a top-level page name (Library). */
    val page = display(24.sp, 28.sp)
    /** 40 px: the title in the player's top bar. */
    val stageTitle = display(20.sp, 22.sp)
    /** 36 px: a title drawn in place of missing artwork (a page's poster). */
    val artTitle = heading(18.sp, 21.sp)
    /** 30 px: the same on a still in a row (continue watching). */
    val artTitleStill = heading(15.sp, 17.sp)
    /** 24 px: the same on a poster in a row or a grid. */
    val artTitleSmall = heading(12.sp, 14.sp)
    /** 36 px: a side panel's or a section's large heading. */
    val panel = heading(18.sp, 22.sp)
    /** 32 px: a row or section heading. */
    val section = heading(16.sp, 20.sp)
    /** 28 px: a heading inside a framed group. */
    val group = heading(14.sp, 17.sp)

    /** 24/32 px: body text, a fact's value. */
    val body = text(12.sp, 16.sp)
    /** 24 px semibold: a card's title line. */
    val bodyStrong = text(12.sp, 16.sp, FontWeight.SemiBold)
    /** Long text in a framed panel (release notes): 12 sp, the floor for reading at a distance (board 21 px). */
    val reading = text(12.sp, 17.sp)
    /** 26 px medium: a large action's label. */
    val action = text(13.sp, 13.sp, FontWeight.Medium)
    /** 24 px medium: a tab, a regular action, a side-panel option. */
    val control = text(12.sp, 12.sp, FontWeight.Medium)
    /** 22 px medium: a pill, a small action. */
    val controlSmall = text(11.sp, 11.sp, FontWeight.Medium)
    /** 28/34 px medium: a recent search's words. */
    val controlLarge = text(14.sp, 17.sp, FontWeight.Medium)
    /** 32/40 px medium: what is typed in a field. */
    val input = text(16.sp, 20.sp, FontWeight.Medium)
    /** 28 px medium: a field's label while it is empty. */
    val inputHint = text(14.sp, 20.sp, FontWeight.Medium)
    /** 22/28 px medium, tabular: the muted meta line everywhere (`.tm`). */
    val meta = text(11.sp, 14.sp, FontWeight.Medium, tabular = true)
    /** 26/34 px medium, tabular: the larger meta line under a hero title. */
    val metaLarge = text(13.sp, 17.sp, FontWeight.Medium, tabular = true)
    /** The smallest meta (grid card facts), medium, tabular: 10 sp floor (board 18 px). */
    val metaSmall = text(10.sp, 13.sp, FontWeight.Medium, tabular = true)
    /** 40 px medium, tabular: a figure beside a [stageTitle] (a channel's number). */
    val stageFigure = text(20.sp, 22.sp, FontWeight.Medium, tabular = true)
    /** 26 px medium, tabular: the player's clock figures. */
    val figure = text(13.sp, 13.sp, FontWeight.Medium, tabular = true)
    /** 20 px medium, uppercase at call site via [Eyebrow]: a kicker above a title. */
    val eyebrow = text(10.sp, 12.sp, FontWeight.Medium).copy(letterSpacing = 0.06.em)
    /** 20 px medium: a chip's words. */
    val chip = text(10.sp, 10.sp, FontWeight.Medium)
    /** A key cap, semibold: 10 sp floor (board 18 px). */
    val key = text(10.sp, 10.sp, FontWeight.SemiBold)
    /** 20 px: a release or file name. */
    val mono = TextStyle(fontFamily = FontMono, fontSize = 10.sp, lineHeight = 13.sp)
    /** 34 px Borel: the "Iris" wordmark. */
    val brand = TextStyle(fontFamily = Borel, fontSize = 17.sp, lineHeight = 17.sp)
    /** 52 px Borel: the wordmark on a page of its own (pairing, setup). */
    val brandLarge = brand.copy(fontSize = 26.sp, lineHeight = 26.sp)

    /**
     * [display] (a Fraunces title style) for [text] when it is a short title, without a figure;
     * else the same size in Cal Sans: a year, a season, a raw name, a sentence.
     */
    fun titleFor(text: String, display: TextStyle): TextStyle =
        if (text.length <= SHORT_TITLE && text.none(Char::isDigit)) {
            display
        } else {
            display.copy(fontFamily = CalSans, fontWeight = FontWeight.SemiBold, letterSpacing = TextUnit.Unspecified)
        }

    /** Longer reads as a sentence, not a title. */
    private const val SHORT_TITLE = 48
}

/**
 * tv-material's [androidx.tv.material3.MaterialTheme.typography], mapped onto
 * [IrisType] so a legacy `MaterialTheme.typography.x` already reads in the
 * new faces. New code uses [IrisType] directly. Lazy: [IrisType] is built from this file's
 * fonts, so reading it while this file initializes would see it half built.
 */
val IrisTvTypography: TvTypography by lazy {
    TvTypography(
        displayLarge = IrisType.hero,
        displayMedium = IrisType.title,
        displaySmall = IrisType.page,
        headlineLarge = IrisType.stageTitle,
        headlineMedium = IrisType.panel,
        headlineSmall = IrisType.section,
        titleLarge = IrisType.group,
        titleMedium = IrisType.bodyStrong.copy(fontSize = 13.sp, lineHeight = 17.sp),
        titleSmall = IrisType.control,
        bodyLarge = IrisType.metaLarge.copy(fontWeight = FontWeight.Normal, fontFeatureSettings = null),
        bodyMedium = IrisType.body,
        bodySmall = IrisType.meta,
        labelLarge = IrisType.controlSmall,
        labelMedium = IrisType.eyebrow,
        labelSmall = IrisType.key,
    )
}
