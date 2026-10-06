@file:Suppress("DEPRECATION")

package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.tv.material3.Border
import androidx.tv.material3.CardBorder
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.CardGlow
import androidx.tv.material3.CardScale
import androidx.tv.material3.CardShape
import androidx.tv.material3.ExperimentalTvMaterial3Api
import studio.kahn.iris.tv.ui.theme.IrisColor
import studio.kahn.iris.tv.ui.theme.IrisFocus
import studio.kahn.iris.tv.ui.theme.IrisShape

// Pre-redesign tv-material Card styling, kept only so the old screens
// compile. DELETE with the last screen using it.

@Deprecated("redesign: use PosterCard / StillCard")
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun irisPosterShape(shape: Shape = IrisShape.card): CardShape = CardDefaults.shape(shape = shape)

@Deprecated("redesign: use PosterCard / StillCard")
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun irisPosterScale(): CardScale = CardDefaults.scale(focusedScale = IrisFocus.cardScale)

@Deprecated("redesign: use PosterCard / StillCard")
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun irisPosterBorder(shape: Shape = IrisShape.card): CardBorder = CardDefaults.border(
    focusedBorder = Border(border = BorderStroke(IrisFocus.ringWidth, IrisColor.accent), shape = shape),
)

@Deprecated("redesign: use PosterCard / StillCard")
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun irisPosterGlow(): CardGlow = CardDefaults.glow()

@Deprecated("redesign: use Artwork (its fallback tile)")
fun irisPosterPlaceholder(): Brush = SolidColor(IrisColor.art)
