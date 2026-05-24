package studio.kahn.iris.tv.ui.theme

import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.Typography as TvTypography
import studio.kahn.iris.tv.R

/**
 * Three families, lifted straight from the web design system so Web and TV
 * read as the same product (see the design doc / `web` Tailwind tokens):
 *
 *   * **Cal Sans** — the display face. Tight, characterful, used only for
 *     hero / section / poster titles. Ships a single drawn weight, so every
 *     [FontWeight] maps to the one file; we never fake-bold it.
 *   * **Inter** — the workhorse sans for body copy, buttons, metadata.
 *     Four real weights (400/500/600/700) bundled as static instances so it
 *     renders identically on an AOSP box with no Play Services (no reliance
 *     on Downloadable Fonts).
 *   * **JetBrains Mono** — the mono accent for technical metadata: file
 *     names, sizes, timecodes, "4K · HDR" pills, keyboard hints.
 *
 * All bundled in `res/font` (≈2.5 MB total) rather than fetched at runtime
 * so the typography is guaranteed present on every TV, online or not.
 */
val CalSans = FontFamily(
    Font(R.font.cal_sans_regular, FontWeight.Normal),
    Font(R.font.cal_sans_regular, FontWeight.Medium),
    Font(R.font.cal_sans_regular, FontWeight.SemiBold),
    Font(R.font.cal_sans_regular, FontWeight.Bold),
)

val Inter = FontFamily(
    Font(R.font.inter_regular, FontWeight.Normal),
    Font(R.font.inter_medium, FontWeight.Medium),
    Font(R.font.inter_semibold, FontWeight.SemiBold),
    Font(R.font.inter_bold, FontWeight.Bold),
)

val JetBrainsMono = FontFamily(
    Font(R.font.jetbrains_mono_regular, FontWeight.Normal),
    Font(R.font.jetbrains_mono_medium, FontWeight.Medium),
    Font(R.font.jetbrains_mono_semibold, FontWeight.SemiBold),
)

/** Convenience aliases mirroring the web `--font-*` custom properties. */
val FontDisplay = CalSans
val FontSans = Inter
val FontMono = JetBrainsMono

// 10-foot typography. Display roles use Cal Sans with the tight tracking the
// web hero carries (`letter-spacing: -0.03em`); everything else is Inter.
// Sizes stay restrained vs. the 1920px web mock — Compose text renders at the
// panel's real dp, and a Mi Box at 1280×720 must not wrap a hero onto three
// lines. The body/title scale matches the previous TV tuning so existing
// screens keep their rhythm; only the family + display tracking change.
@OptIn(ExperimentalTvMaterial3Api::class)
val IrisTvTypography = TvTypography(
    displayLarge = TextStyle(fontFamily = CalSans, fontSize = 64.sp, lineHeight = 64.sp, fontWeight = FontWeight.Normal, letterSpacing = (-1.8).sp),
    displayMedium = TextStyle(fontFamily = CalSans, fontSize = 52.sp, lineHeight = 54.sp, fontWeight = FontWeight.Normal, letterSpacing = (-1.4).sp),
    displaySmall = TextStyle(fontFamily = CalSans, fontSize = 40.sp, lineHeight = 44.sp, fontWeight = FontWeight.Normal, letterSpacing = (-1.0).sp),
    headlineLarge = TextStyle(fontFamily = CalSans, fontSize = 34.sp, lineHeight = 40.sp, fontWeight = FontWeight.Normal, letterSpacing = (-0.8).sp),
    headlineMedium = TextStyle(fontFamily = CalSans, fontSize = 28.sp, lineHeight = 34.sp, fontWeight = FontWeight.Normal, letterSpacing = (-0.5).sp),
    headlineSmall = TextStyle(fontFamily = CalSans, fontSize = 24.sp, lineHeight = 30.sp, fontWeight = FontWeight.Normal, letterSpacing = (-0.3).sp),
    titleLarge = TextStyle(fontFamily = Inter, fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-0.2).sp),
    titleMedium = TextStyle(fontFamily = Inter, fontSize = 18.sp, lineHeight = 24.sp, fontWeight = FontWeight.Medium, letterSpacing = (-0.1).sp),
    titleSmall = TextStyle(fontFamily = Inter, fontSize = 16.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-0.1).sp),
    bodyLarge = TextStyle(fontFamily = Inter, fontSize = 18.sp, lineHeight = 28.sp, fontWeight = FontWeight.Normal),
    bodyMedium = TextStyle(fontFamily = Inter, fontSize = 16.sp, lineHeight = 24.sp, fontWeight = FontWeight.Normal),
    bodySmall = TextStyle(fontFamily = Inter, fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.Normal),
    // Label roles carry the eyebrow tracking (uppercase + wide letter-spacing)
    // applied at call sites; the base style stays Inter SemiBold.
    labelLarge = TextStyle(fontFamily = Inter, fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 1.6.sp),
    labelMedium = TextStyle(fontFamily = Inter, fontSize = 13.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 1.4.sp),
    labelSmall = TextStyle(fontFamily = JetBrainsMono, fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium, letterSpacing = 0.4.sp),
)
