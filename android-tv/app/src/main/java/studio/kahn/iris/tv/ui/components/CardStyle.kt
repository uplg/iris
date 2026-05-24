package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Border
import androidx.tv.material3.CardBorder
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.CardGlow
import androidx.tv.material3.CardScale
import androidx.tv.material3.CardShape
import androidx.tv.material3.ExperimentalTvMaterial3Api
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Radius

/**
 * Shared focus styling for poster-style [androidx.tv.material3.Card]s, so every
 * shelf / grid card across the app lifts the same way on D-pad focus: the
 * design's brand ring + brand glow + a small pop (web `.card[data-focused]`).
 *
 * Screens keep their own card *content* (poster image, TMDB plumbing, badges)
 * and just spread these four into the `Card(...)` call — `shape`, `scale`,
 * `border`, `glow` — to inherit the look. Plain (non-composable) builders so
 * they're cheap to call inline.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun irisPosterShape(shape: Shape = RoundedCornerShape(Radius.poster)): CardShape =
    CardDefaults.shape(shape = shape)

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun irisPosterScale(): CardScale =
    CardDefaults.scale(focusedScale = Focus.posterScale)

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun irisPosterBorder(shape: Shape = RoundedCornerShape(Radius.poster)): CardBorder =
    CardDefaults.border(
        focusedBorder = Border(
            border = BorderStroke(Focus.ring, IrisColors.Brand),
            shape = shape,
        ),
    )

/**
 * No focus glow. A brand drop-glow behind a focused surface muddied text and
 * read as heavy on a 10-foot panel, so focus is signalled by the ring + lift
 * alone. Kept as a named no-op so call sites stay uniform (and a glow can be
 * reintroduced in one place if ever wanted).
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun irisPosterGlow(): CardGlow = CardDefaults.glow()

/**
 * The no-artwork poster fill — a brand-tinted diagonal wash fading to the page
 * ground, matching the web `.poster .fallback`. Used behind the title when a
 * card has no TMDB poster.
 */
fun irisPosterPlaceholder(): Brush = Brush.linearGradient(
    0.0f to IrisColors.Brand.copy(alpha = 0.32f),
    0.55f to IrisColors.Brand2.copy(alpha = 0.16f),
    1.0f to IrisColors.BackgroundDeep,
)
