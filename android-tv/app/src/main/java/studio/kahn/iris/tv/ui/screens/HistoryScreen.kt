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
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.ColorMatrix
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
import studio.kahn.iris.tv.data.ResolveBody
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
 * One display group of the history log: a collection ("ghost" when every
 * source torrent was reclaimed) and the episodes the user watched in it.
 * Standalone rows without episode coordinates (movies) render merged.
 */
private data class HistoryGroup(
    val key: String,
    val collectionId: String?,
    val title: String,
    val tmdbId: Long?,
    val kind: String?,
    /** All rows deleted → the whole collection is a ghost. */
    val ghost: Boolean,
    val items: List<HistoryItem>,
) {
    /** Movie-style single line: no episode coordinates to list under a header. */
    val solo: Boolean
        get() = items.size == 1 && items[0].season == null && items[0].absoluteEpisode == null
}

private fun buildGroups(items: List<HistoryItem>): List<HistoryGroup> {
    val order = LinkedHashMap<String, MutableList<HistoryItem>>()
    for (it in items) {
        val key = it.collectionId?.toString() ?: "solo:${it.infohash}"
        order.getOrPut(key) { mutableListOf() }.add(it)
    }
    return order.map { (key, rows) ->
        val first = rows.first()
        HistoryGroup(
            key = key,
            collectionId = first.collectionId?.toString(),
            title = first.collectionTitle ?: first.torrentName,
            tmdbId = first.tmdbId,
            kind = first.kind?.value,
            ghost = rows.all { it.deleted },
            items = rows,
        )
    }
}

/** "S01E03" / "Episode 1156" / file leaf — what exactly was watched. */
private fun episodeLabel(item: HistoryItem): String? {
    item.absoluteEpisode?.let { return "Episode $it" }
    val s = item.season
    val e = item.episode
    if (s != null && e != null) {
        return if (e == 0L) "Season $s" else "S%02dE%02d".format(s, e)
    }
    return item.filePath?.substringAfterLast('/')
}

private fun statusLine(item: HistoryItem): String {
    if (item.completed) return "Watched"
    val pct = item.durationSeconds?.takeIf { it > 0 }
        ?.let { ((item.positionSeconds / it) * 100).toInt().coerceIn(0, 100) }
    return if (pct != null && pct > 0) "$pct% watched" else "Just started"
}

private fun canRestore(item: HistoryItem): Boolean =
    item.deleted && item.sourceProvider != null && item.sourceExternalId != null

/**
 * Full watch history grouped by collection — the "ghost collections"
 * design, 1:1 with the web page: every show/movie the user touched stays
 * listed under its clean title + poster even after the disk-reclaim GC
 * removed all of its torrents (collections are never hard-deleted server-
 * side, and grouping derives from the caller's OWN history, so ghosts are
 * per-user by construction). Headers navigate to the collection page
 * (indexer offers → re-grab); deleted rows offer "Download again", which
 * re-ingests the exact same release — same infohash, so the stored resume
 * position applies untouched. Reachable from Settings.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun HistoryScreen(
    container: AppContainer,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onOpenCollection: (collectionId: String) -> Unit,
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
    var restoringKey by remember { mutableStateOf<String?>(null) }

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

    fun restore(item: HistoryItem) {
        val provider = item.sourceProvider ?: return
        val externalId = item.sourceExternalId ?: return
        val key = "${item.infohash}:${item.fileIdx}"
        if (restoringKey != null) return
        restoringKey = key
        scope.launch {
            try {
                val url = container.sessionStore.serverUrl.first()
                    ?: run { error = "Not signed in"; return@launch }
                container.apiFor(url).ingest(
                    ResolveBody(
                        providerId = provider,
                        externalId = externalId,
                        tmdbId = item.tmdbId,
                    ),
                )
                // Straight back into playback: the stream path serves while
                // the torrent downloads and the saved progress row resumes
                // the position — "reprend exactement où il était".
                onPickFile(item.infohash, item.fileIdx.toInt())
            } catch (e: Exception) {
                error = e.message ?: "Restore failed"
            } finally {
                restoringKey = null
            }
        }
    }

    LaunchedEffect(Unit) {
        loading = true
        load(1)
        loading = false
    }

    val groups = remember(items) { buildGroups(items) }

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
                for (group in groups) {
                    if (group.solo) {
                        items(group.items, key = { "solo:${it.infohash}:${it.fileIdx}" }) { item ->
                            SoloRow(
                                container = container,
                                group = group,
                                item = item,
                                restoringKey = restoringKey,
                                onPlay = { onPickFile(item.infohash, item.fileIdx.toInt()) },
                                onOpenCollection = onOpenCollection,
                                onRestore = ::restore,
                            )
                        }
                    } else {
                        item(key = "h:${group.key}") {
                            GroupHeader(
                                container = container,
                                group = group,
                                onOpenCollection = onOpenCollection,
                            )
                        }
                        items(group.items, key = { "e:${it.infohash}:${it.fileIdx}" }) { item ->
                            EpisodeRow(
                                item = item,
                                restoringKey = restoringKey,
                                onPlay = { onPickFile(item.infohash, item.fileIdx.toInt()) },
                                onRestore = ::restore,
                            )
                        }
                    }
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

/** Small TMDB poster resolved through the shared metadata endpoint. */
@Composable
private fun GroupPoster(
    container: AppContainer,
    tmdbId: Long?,
    kind: String?,
    ghost: Boolean,
    contentDescription: String,
) {
    var meta by remember(tmdbId) { mutableStateOf<MediaMetadata?>(null) }
    LaunchedEffect(tmdbId, kind) {
        val id = tmdbId ?: return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        meta = runCatching { container.apiFor(url).tmdbMetadata(id, kind) }.getOrNull()
    }
    val posterUrl = tmdbPosterUrl(meta?.posterPath, "w185")
    Box(
        Modifier
            .width(42.dp)
            .aspectRatio(2f / 3f)
            .clip(RoundedCornerShape(6.dp)),
        contentAlignment = Alignment.Center,
    ) {
        if (posterUrl != null) {
            AsyncImage(
                model = posterUrl,
                contentDescription = contentDescription,
                modifier = Modifier.fillMaxSize().let { if (ghost) it.alpha(0.45f) else it },
                contentScale = ContentScale.Crop,
                colorFilter = if (ghost) {
                    ColorFilter.colorMatrix(ColorMatrix().apply { setToSaturation(0f) })
                } else {
                    null
                },
            )
        } else {
            Box(Modifier.fillMaxSize().background(IrisColors.Overlay12))
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun GhostPill() {
    Surface(
        shape = RoundedCornerShape(4.dp),
        colors = SurfaceDefaults.colors(containerColor = IrisColors.Overlay12),
    ) {
        Text(
            "GONE FROM DISK",
            style = MaterialTheme.typography.labelSmall,
            color = IrisColors.MutedForeground,
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 1.dp),
        )
    }
}

/** Collection header: poster + clean display title; ghosts stay listed,
 *  greyed. Clicking navigates to the collection page (re-grab there). */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun GroupHeader(
    container: AppContainer,
    group: HistoryGroup,
    onOpenCollection: (collectionId: String) -> Unit,
) {
    val rowShape = RoundedCornerShape(Radius.button)
    val interactive = group.collectionId != null
    Surface(
        onClick = { group.collectionId?.let(onOpenCollection) },
        modifier = Modifier.fillMaxWidth().padding(top = Spacing.sm),
        enabled = interactive,
        shape = ClickableSurfaceDefaults.shape(shape = rowShape),
        scale = ClickableSurfaceDefaults.scale(focusedScale = 1f),
        colors = ClickableSurfaceDefaults.colors(
            containerColor = Color.Transparent,
            focusedContainerColor = IrisColors.Overlay12,
            disabledContainerColor = Color.Transparent,
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
            GroupPoster(container, group.tmdbId, group.kind, group.ghost, group.title)
            Text(
                group.title,
                style = MaterialTheme.typography.titleMedium,
                color = if (group.ghost) IrisColors.MutedForeground
                else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (group.ghost) GhostPill()
        }
    }
}

/** Per-episode line under a header: "S01E03 · 43% watched · 2d ago". */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EpisodeRow(
    item: HistoryItem,
    restoringKey: String?,
    onPlay: () -> Unit,
    onRestore: (HistoryItem) -> Unit,
) {
    val playable = !item.deleted
    val label = episodeLabel(item) ?: item.torrentName
    val rowShape = RoundedCornerShape(Radius.button)
    Row(
        Modifier.fillMaxWidth().padding(start = Spacing.lg),
        horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Surface(
            onClick = onPlay,
            modifier = Modifier.weight(1f),
            enabled = playable,
            shape = ClickableSurfaceDefaults.shape(shape = rowShape),
            scale = ClickableSurfaceDefaults.scale(focusedScale = 1f),
            colors = ClickableSurfaceDefaults.colors(
                containerColor = IrisColors.Overlay06,
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
                Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    label,
                    style = MaterialTheme.typography.titleSmall,
                    color = if (playable) MaterialTheme.colorScheme.onSurface
                    else MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.width(110.dp),
                )
                Text(
                    statusLine(item),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    formatRecentTime(item.lastWatchedAt),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (canRestore(item)) {
            val busy = restoringKey == "${item.infohash}:${item.fileIdx}"
            IrisButton(
                if (busy) "Restoring…" else "Download again",
                { onRestore(item) },
                variant = IrisButtonVariant.Ghost,
                enabled = !busy && restoringKey == null,
            )
        }
    }
}

/** Standalone (movie) line — header and row merged into one. */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SoloRow(
    container: AppContainer,
    group: HistoryGroup,
    item: HistoryItem,
    restoringKey: String?,
    onPlay: () -> Unit,
    onOpenCollection: (collectionId: String) -> Unit,
    onRestore: (HistoryItem) -> Unit,
) {
    val playable = !item.deleted
    val openable = group.collectionId != null
    val rowShape = RoundedCornerShape(Radius.button)
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Surface(
            onClick = {
                when {
                    playable -> onPlay()
                    // `openable` smart-casts `collectionId` to non-null.
                    openable -> onOpenCollection(group.collectionId)
                }
            },
            modifier = Modifier.weight(1f),
            enabled = playable || openable,
            shape = ClickableSurfaceDefaults.shape(shape = rowShape),
            scale = ClickableSurfaceDefaults.scale(focusedScale = 1f),
            colors = ClickableSurfaceDefaults.colors(
                containerColor = if (item.deleted) IrisColors.Card.copy(alpha = 0.5f)
                else IrisColors.Overlay06,
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
                GroupPoster(container, group.tmdbId, group.kind, item.deleted, group.title)
                Column(
                    Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(3.dp),
                ) {
                    Text(
                        group.title,
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
                            statusLine(item),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        if (item.deleted) GhostPill()
                    }
                }
                Text(
                    formatRecentTime(item.lastWatchedAt),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (canRestore(item)) {
            val busy = restoringKey == "${item.infohash}:${item.fileIdx}"
            IrisButton(
                if (busy) "Restoring…" else "Download again",
                { onRestore(item) },
                variant = IrisButtonVariant.Ghost,
                enabled = !busy && restoringKey == null,
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
