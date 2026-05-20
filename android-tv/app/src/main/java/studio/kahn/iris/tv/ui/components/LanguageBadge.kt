package studio.kahn.iris.tv.ui.components

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.Text

/**
 * Compact FR / EN / `MULTi` pill rendered next to search results and
 * available-episode rows so the household's anglophone + francophone
 * users can pick the language they recognise at a glance.
 *
 * Returns nothing (composable no-op) for unknown / null tags — every
 * other indexer release is "MULTi" or "FR" anyway, and a row of
 * placeholder "UNKNOWN" pills would just be noise.
 *
 * Colour mapping is intentionally stable so users learn the
 * shorthand (don't reshuffle):
 *   - FR    : sky-400 — calm, matches the French tricolour tone
 *   - EN    : amber-500 — warm contrast against FR's cool blue
 *   - MULTi : emerald-500 — the "satisfies both" tag
 *
 * Mirrors `web/src/components/LanguageBadge.tsx` so the same
 * language gets the same colour across Web and TV; users moving
 * between the two surfaces don't have to relearn.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun LanguageBadge(language: String?, modifier: Modifier = Modifier) {
    if (language.isNullOrBlank() || language.equals("unknown", ignoreCase = true)) {
        return
    }
    val (label, color) = when (language.lowercase()) {
        "french" -> "FR" to Color(0xFF38BDF8)
        "english" -> "EN" to Color(0xFFF59E0B)
        "multi" -> "MULTi" to Color(0xFF10B981)
        // Future tags stay visible — show them verbatim with a neutral
        // pill, rather than silently dropping. Better a stray "X-FR"
        // than a blank row when an indexer ships a new convention.
        else -> language.uppercase() to Color(0xFF52525B)
    }
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(4.dp),
        colors = SurfaceDefaults.colors(containerColor = color.copy(alpha = 0.9f)),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = Color.White,
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
        )
    }
}

/** Padding-free variant for tight rows. Same renderer, just lets the
 *  caller control margins via `Modifier`. */
@Composable
fun LanguageBadgeCompact(language: String?, modifier: Modifier = Modifier) {
    LanguageBadge(language = language, modifier = modifier.padding(PaddingValues(0.dp)))
}
