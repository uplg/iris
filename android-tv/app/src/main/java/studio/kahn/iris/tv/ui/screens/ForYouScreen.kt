package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.ForYou
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.Spacing

/**
 * The organized "For You" page — the blended top picks plus per-genre /
 * "because you watched" / new-anime sections (`/api/me/for-you/page`).
 * Reachable from the home nav and the home shelf's "See all →".
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun ForYouScreen(
    container: AppContainer,
    onOpenCollection: (String) -> Unit,
    onPickResult: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    onOpenSearch: (String) -> Unit,
) {
    var data by remember { mutableStateOf<ForYou?>(null) }
    var loading by remember { mutableStateOf(true) }

    LaunchedEffect(Unit) {
        val url = container.sessionStore.serverUrl.first()
        data = if (url != null) {
            withContext(Dispatchers.IO) {
                runCatching { container.apiFor(url).forYouPage() }.getOrNull()
            }
        } else {
            null
        }
        loading = false
    }

    val shelves = data?.shelves.orEmpty()

    // No background here — DiscoverScreen paints the Background +
    // ambient once for the whole page (an opaque repaint under the tab
    // strip rendered as a flat black band).
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(vertical = Spacing.xl),
        verticalArrangement = Arrangement.spacedBy(Spacing.xxl),
    ) {
        // No header block: this renders as the "For You" TAB inside
        // DiscoverScreen, whose tab row already names the page.
        if (loading) {
            item(key = "loading") {
                Text(
                    "Loading…",
                    style = MaterialTheme.typography.bodyLarge,
                    color = IrisColors.FgDim,
                    modifier = Modifier.padding(horizontal = Spacing.gutter),
                )
            }
        } else if (shelves.isEmpty()) {
            item(key = "empty") {
                Text(
                    "Nothing to recommend yet. Set your preferences and check back once the catalogue has refreshed.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = IrisColors.FgDim,
                    modifier = Modifier.padding(horizontal = Spacing.gutter),
                )
            }
        } else {
            shelves.forEach { shelf ->
                item(key = "shelf-${shelf.key}") {
                    Shelf(title = shelf.title) {
                        items(shelf.items, key = { it.catalogId }) { card ->
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
