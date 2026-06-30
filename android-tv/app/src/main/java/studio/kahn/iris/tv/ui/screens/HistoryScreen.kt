package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Border
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.HistoryItem
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing

private const val PAGE_SIZE = 30

/**
 * Full watch history — in-progress AND completed, one row per episode,
 * including titles whose source torrent has since been deleted
 * (disk-reclaim GC, admin cleanup). Distinct from the Home shelf's
 * "Continue watching" (which only shows unfinished items and drops deleted
 * ones — a deleted file can't be resumed) — this screen is the dedicated
 * "where was I, even after a cleanup" answer. List, not a poster grid: a
 * household member watching several shows in parallel needs to scan a log,
 * not page through cards. `LazyColumn` only composes visible rows, so this
 * stays smooth at any history length without extra plumbing. Reachable
 * from Settings.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun HistoryScreen(
    container: AppContainer,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onBack: () -> Unit,
) {
    val layout = LocalTvLayout.current
    val scope = rememberCoroutineScope()
    var items by remember { mutableStateOf<List<HistoryItem>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var loadingMore by remember { mutableStateOf(false) }
    var page by remember { mutableIntStateOf(1) }
    var hasMore by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }

    suspend fun load(targetPage: Int) {
        val url = container.sessionStore.serverUrl.first()
        if (url == null) {
            error = "Not signed in"
            return
        }
        val fetched = runCatching {
            container.apiFor(url).history(limit = PAGE_SIZE, offset = (targetPage - 1) * PAGE_SIZE)
        }.getOrElse {
            error = it.message ?: "Failed to load history"
            return
        }
        error = null
        items = if (targetPage == 1) fetched else items + fetched
        hasMore = fetched.size >= PAGE_SIZE
        page = targetPage
    }

    LaunchedEffect(Unit) {
        loading = true
        load(1)
        loading = false
    }

    Column(
        Modifier
            .fillMaxSize()
            .padding(horizontal = layout.gutterHorizontal, vertical = layout.gutterVertical),
        verticalArrangement = Arrangement.spacedBy(Spacing.lg),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.md),
        ) {
            IrisButton("← Back", onBack, variant = IrisButtonVariant.Ghost)
            Text(
                "Watch history",
                style = MaterialTheme.typography.displaySmall,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }

        when {
            loading -> Text(
                "Loading…",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            error != null -> Text(
                error!!,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.error,
            )
            items.isEmpty() -> Text(
                "Nothing watched yet.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            else -> LazyColumn(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(Spacing.xs),
            ) {
                items(items, key = { "${it.infohash}:${it.fileIdx}" }) { item ->
                    HistoryRow(
                        container = container,
                        item = item,
                        onClick = { onPickFile(item.infohash, item.fileIdx.toInt()) },
                    )
                }
                if (hasMore) {
                    item(key = "load-more") {
                        Box(
                            Modifier.fillMaxWidth().padding(vertical = Spacing.md),
                            contentAlignment = Alignment.Center,
                        ) {
                            IrisButton(
                                if (loadingMore) "Loading…" else "Load more",
                                {
                                    scope.launch {
                                        loadingMore = true
                                        load(page + 1)
                                        loadingMore = false
                                    }
                                },
                                variant = IrisButtonVariant.Ghost,
                                enabled = !loadingMore,
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
private fun HistoryRow(
    container: AppContainer,
    item: HistoryItem,
    onClick: () -> Unit,
) {
    // `item.tmdbId` is already the COLLECTION's id (authoritative — see
    // `project_collection_tmdb_authority`), but `tmdbVerified` reflects the
    // SOURCE TORRENT's own (often-unset) flag, not the collection's. Gating
    // on it here dropped posters for plenty of legitimately-resolved
    // history rows. Other poster lookups (`HomeScreen`'s Continue Watching
    // row, `LibraryScreen`'s `LibraryGridCard`) only check for a non-null
    // id — match that.
    var meta by remember(item.tmdbId) { mutableStateOf<MediaMetadata?>(null) }
    LaunchedEffect(item.tmdbId, item.kind) {
        val id = item.tmdbId ?: return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        meta = runCatching {
            container.apiFor(url).tmdbMetadata(id, item.kind?.value)
        }.getOrNull()
    }
    val posterUrl = tmdbPosterUrl(meta?.posterPath, "w185")
    val title = item.filePath?.substringAfterLast('/') ?: item.torrentName
    val pct = item.durationSeconds?.takeIf { it > 0 }
        ?.let { ((item.positionSeconds / it).toFloat()).coerceIn(0f, 1f) }
    val subtitle = when {
        item.deleted -> "No longer available"
        item.completed -> "Watched"
        pct != null && pct > 0f -> "${(pct * 100).toInt()}% watched"
        else -> "Just started"
    }
    val rowShape = RoundedCornerShape(Radius.button)
    val interactive = !item.deleted

    // Deleted entries have nothing left to resume — a plain (non-clickable)
    // Surface so they never grab D-pad focus, instead of a row whose click
    // would silently do nothing.
    Surface(
        onClick = if (interactive) onClick else ({}),
        modifier = Modifier.fillMaxWidth(),
        enabled = interactive,
        shape = ClickableSurfaceDefaults.shape(shape = rowShape),
        scale = ClickableSurfaceDefaults.scale(focusedScale = 1f),
        colors = ClickableSurfaceDefaults.colors(
            containerColor = if (item.deleted) IrisColors.Card.copy(alpha = 0.5f) else IrisColors.Overlay06,
            focusedContainerColor = IrisColors.Overlay12,
            disabledContainerColor = IrisColors.Card.copy(alpha = 0.5f),
        ),
        border = ClickableSurfaceDefaults.border(
            border = Border.None,
            focusedBorder = Border(
                androidx.compose.foundation.BorderStroke(Focus.ring, IrisColors.Brand),
                shape = rowShape,
            ),
        ),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(8.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier
                    .width(46.dp)
                    .aspectRatio(2f / 3f)
                    .clip(RoundedCornerShape(6.dp)),
                contentAlignment = Alignment.Center,
            ) {
                if (posterUrl != null) {
                    AsyncImage(
                        model = posterUrl,
                        contentDescription = title,
                        modifier = Modifier.fillMaxSize().let { if (item.deleted) it.alpha(0.5f) else it },
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    Box(
                        Modifier.fillMaxSize().background(
                            IrisColors.Overlay12,
                        ),
                    )
                }
            }
            Column(
                Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        subtitle,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (item.deleted) {
                        Surface(
                            shape = RoundedCornerShape(4.dp),
                            colors = SurfaceDefaults.colors(
                                containerColor = MaterialTheme.colorScheme.error,
                            ),
                        ) {
                            Text(
                                "REMOVED",
                                style = MaterialTheme.typography.labelSmall,
                                color = Color.White,
                                modifier = Modifier.padding(horizontal = 6.dp, vertical = 1.dp),
                            )
                        }
                    }
                }
            }
            Text(
                formatRecentTime(item.lastWatchedAt),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Short "2m ago" / "just now" relative time — mirrors the web's
 *  `formatRecentTime` (lib/format.ts) so the two clients read identically. */
private fun formatRecentTime(at: java.time.OffsetDateTime): String {
    val secs = java.time.Duration.between(at, java.time.OffsetDateTime.now()).seconds.coerceAtLeast(0)
    return when {
        secs < 10 -> "just now"
        secs < 60 -> "${secs}s ago"
        secs < 3600 -> "${secs / 60}m ago"
        secs < 86_400 -> "${secs / 3600}h ago"
        else -> "${secs / 86_400}d ago"
    }
}
