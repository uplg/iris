package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Card
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.MoodResults
import studio.kahn.iris.tv.data.MoodTile
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Spacing

/**
 * The mood board ("Tonight"): a grid of curated mood tiles (taste-ordered, each
 * with a representative backdrop) + a Film/Series toggle. Picking a mood shows
 * its results (catalogue ∪ broad TMDB, recency-filtered to grabbable,
 * taste-ranked). Board ↔ results is internal state. Mirrors the web /moods page.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun MoodsScreen(
    container: AppContainer,
    onOpenCollection: (String) -> Unit,
    onPickResult: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    onOpenSearch: (String) -> Unit,
) {
    var board by remember { mutableStateOf<List<MoodTile>>(emptyList()) }
    var kind by remember { mutableStateOf("movie") }
    var selected by remember { mutableStateOf<MoodTile?>(null) }
    var results by remember { mutableStateOf<MoodResults?>(null) }
    var loadingResults by remember { mutableStateOf(false) }
    // Board ↔ results is internal state — without this, Back from a
    // mood's results pops the whole route instead of the board.
    androidx.activity.compose.BackHandler(enabled = selected != null) { selected = null }

    // The board's genres depend on the kind, so re-fetch when it toggles.
    LaunchedEffect(kind) {
        val url = container.sessionStore.serverUrl.first()
        if (url != null) {
            board = withContext(Dispatchers.IO) {
                runCatching { container.apiFor(url).moodBoard(kind).moods }.getOrNull().orEmpty()
            }
        }
    }

    LaunchedEffect(selected, kind) {
        val mood = selected
        if (mood == null) {
            results = null
            return@LaunchedEffect
        }
        loadingResults = true
        val url = container.sessionStore.serverUrl.first()
        results = if (url != null) {
            withContext(Dispatchers.IO) {
                runCatching { container.apiFor(url).moodResults(mood.id, kind) }.getOrNull()
            }
        } else {
            null
        }
        loadingResults = false
    }

    // No background here — DiscoverScreen paints the Background +
    // ambient once for the whole page (an opaque repaint under the
    // tab strip rendered as a flat black band).
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(vertical = Spacing.xxl),
        verticalArrangement = Arrangement.spacedBy(Spacing.xxl),
    ) {
        item(key = "header") {
            Column(
                modifier = Modifier.padding(horizontal = Spacing.gutter),
                verticalArrangement = Arrangement.spacedBy(Spacing.md),
            ) {
                SectionTitle(selected?.label ?: "What are you in the mood for?")
                Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                    IrisButton(
                        text = "Films",
                        onClick = { kind = "movie" },
                        variant =
                            if (kind == "movie") IrisButtonVariant.Primary else IrisButtonVariant.Ghost,
                    )
                    IrisButton(
                        text = "Series",
                        onClick = { kind = "tv" },
                        variant =
                            if (kind == "tv") IrisButtonVariant.Primary else IrisButtonVariant.Ghost,
                    )
                    if (selected != null) {
                        IrisButton(
                            text = "← Back",
                            onClick = { selected = null },
                            variant = IrisButtonVariant.Ghost,
                        )
                    }
                }
            }
        }

        if (selected == null) {
            board.chunked(3).forEachIndexed { rowIdx, rowTiles ->
                item(key = "mood-row-$rowIdx") {
                    Row(
                        modifier = Modifier.padding(horizontal = Spacing.gutter),
                        horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
                    ) {
                        rowTiles.forEach { tile ->
                            MoodTileCard(tile = tile, onClick = { selected = tile })
                        }
                    }
                }
            }
        } else if (loadingResults) {
            item(key = "loading") {
                Text(
                    "Finding something good…",
                    style = MaterialTheme.typography.bodyLarge,
                    color = IrisColors.FgDim,
                    modifier = Modifier.padding(horizontal = Spacing.gutter),
                )
            }
        } else {
            val items = results?.items.orEmpty()
            if (items.isEmpty()) {
                item(key = "empty") {
                    Text(
                        "Nothing grabbable for this mood right now.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = IrisColors.FgDim,
                        modifier = Modifier.padding(horizontal = Spacing.gutter),
                    )
                }
            } else {
                item(key = "results") {
                    Shelf(title = selected?.label ?: "Mood") {
                        items(items, key = { it.catalogId }) { card ->
                            CatalogCardTv(
                                container = container,
                                card = card,
                                onClick = {
                                    routeCatalogClick(card, onOpenCollection, onPickResult, onOpenSearch)
                                },
                            )
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun MoodTileCard(tile: MoodTile, onClick: () -> Unit) {
    Card(
        onClick = onClick,
        modifier = Modifier.width(300.dp).aspectRatio(16f / 10f),
        colors = CardDefaults.colors(containerColor = IrisColors.Card),
    ) {
        Box(Modifier.fillMaxSize()) {
            val url = tile.backdropUrl
            if (url != null) {
                AsyncImage(
                    model = url,
                    contentDescription = tile.label,
                    modifier = Modifier.fillMaxSize(),
                    contentScale = ContentScale.Crop,
                )
            }
            // Legibility scrim under the label.
            Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.35f)))
            Box(
                Modifier.fillMaxSize().padding(Spacing.md),
                contentAlignment = Alignment.BottomStart,
            ) {
                Text(
                    tile.label,
                    style = MaterialTheme.typography.titleMedium,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                )
            }
        }
    }
}
