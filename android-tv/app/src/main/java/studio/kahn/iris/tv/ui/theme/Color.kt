package studio.kahn.iris.tv.ui.theme

import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color

/**
 * The Iris palette, ported 1:1 from the web design system's OKLCH tokens
 * (`web` `styles` `:root` + `[data-accent="violet"]`). OKLCH was converted to
 * sRGB offline so the TV reads as the same product as the web app — same
 * slate-violet ground, same indigo→violet→fuchsia brand ramp.
 *
 * Single source of truth: `Theme.kt` feeds these into both the TV-Material and
 * stock-Material3 color schemes, and components read the brand ramp / glow /
 * elevation tints directly from here for things the Material `ColorScheme`
 * can't express (gradients, focus glow, layered surfaces).
 */
object IrisColors {
    // ── Ground & ink ────────────────────────────────────────────────────────
    val Background = Color(0xFF0B0C10)      // oklch(0.155 0.008 270)
    val BackgroundDeep = Color(0xFF07080B)  // oklch(0.135 0.008 270) — gradient floor
    val Foreground = Color(0xFFF9FAFC)      // oklch(0.985 0.003 270)
    val Card = Color(0xFF131418)            // oklch(0.192 0.009 270) — base surface / elev
    val Elev2 = Color(0xFF1C1E24)           // oklch(0.235 0.012 270) — raised surface
    val MutedForeground = Color(0xFFA2A4AA) // oklch(0.72 0.008 270)
    val FgDim = Color(0xFF72747B)           // oklch(0.56 0.01 270) — captions, mono subs

    // Hairlines are pure-white at low alpha, exactly like the web
    // `--border` / `--border-strong` (`oklch(1 0 0 / 0.08|0.18)`).
    val Border = Color(0xFFFFFFFF).copy(alpha = 0.08f)
    val BorderStrong = Color(0xFFFFFFFF).copy(alpha = 0.18f)
    val Overlay06 = Color(0xFFFFFFFF).copy(alpha = 0.06f) // ghost button fill
    val Overlay12 = Color(0xFFFFFFFF).copy(alpha = 0.12f) // ghost button focus fill

    // ── Brand ramp (violet accent — the design default) ─────────────────────
    val Brand = Color(0xFFA58DFF)           // oklch(0.72 0.18 290)
    val BrandHi = Color(0xFFB199FF)         // brand l+0.04 — primary button top stop
    val Brand2 = Color(0xFF69C1FC)          // oklch(0.78 0.12 240) — cool end
    val Brand3 = Color(0xFFF08FE8)          // oklch(0.78 0.16 330) — warm end
    val OnBrand = Color(0xFF0C0D12)         // oklch(0.16 0.01 270) — ink on brand fills
    val BrandSoft = Brand.copy(alpha = 0.18f)
    val BrandGlow = Brand.copy(alpha = 0.45f)

    // ── Status ──────────────────────────────────────────────────────────────
    val Success = Color(0xFF55C483)         // oklch(0.74 0.14 155)
    val Warn = Color(0xFFFFB330)            // oklch(0.82 0.16 75)
    val Destructive = Color(0xFFFF4C4D)     // oklch(0.68 0.22 25)
}

/**
 * The signature indigo→violet→fuchsia wordmark/heading gradient
 * (`--brand-text`, `linear-gradient(105deg, brand-3, brand, brand-2)`).
 * 105° ≈ left→right with a slight downward tilt; for a single line of text a
 * horizontal sweep reads identically and survives any width.
 */
fun irisBrandGradient(): Brush = Brush.linearGradient(
    0.0f to IrisColors.Brand3,
    0.5f to IrisColors.Brand,
    1.0f to IrisColors.Brand2,
)

/**
 * Primary-button fill — the subtle top-down sheen the web uses
 * (`linear-gradient(180deg, brand l+0.04, brand)`), giving the flat brand a
 * touch of dimensionality without a hard bevel.
 */
fun irisPrimaryFill(): Brush = Brush.verticalGradient(
    0.0f to IrisColors.BrandHi,
    1.0f to IrisColors.Brand,
)

/**
 * Ambient backlight wash for the home backdrop (`.ambient`): a soft violet
 * glow biased to the upper-right plus a cooler pool at the lower-left, fading
 * to the page ground. Purely decorative — no content depends on it.
 */
fun irisAmbient(): Brush = Brush.linearGradient(
    0.0f to IrisColors.Brand.copy(alpha = 0.14f),
    0.45f to IrisColors.Background,
    1.0f to IrisColors.Brand2.copy(alpha = 0.10f),
)
