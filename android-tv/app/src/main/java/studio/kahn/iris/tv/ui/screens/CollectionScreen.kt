package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Button
import androidx.tv.material3.ButtonDefaults
import androidx.tv.material3.Card
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.first
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CollectionDetail
import studio.kahn.iris.tv.data.CollectionEpisode
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.TmdbMetadata
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Spacing

private val VIDEO_EXTS_C = listOf(
    ".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv",
)

/**
 * Collection browse view. Mirrors the web's `/collection/:id` page:
 *
 *   * TV-kind: shows the merged episode grid joined across every
 *     torrent in the collection (`episode_files`). Picking a row
 *     jumps to /watch.
 *   * Movie / no SCENE-parsed episodes: shows every playable file
 *     across the collection's torrents instead.
 *
 * Reached by clicking a CollectionCard on the home Library shelf.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun CollectionScreen(
    container: AppContainer,
    collectionId: String,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onBack: () -> Unit,
) {
    val layout = LocalTvLayout.current
    var detail by remember(collectionId) { mutableStateOf<CollectionDetail?>(null) }
    var meta by remember(collectionId) { mutableStateOf<TmdbMetadata?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(collectionId) {
        try {
            val url = container.sessionStore.serverUrl.first()
            if (url == null) {
                error = "Not signed in"
                return@LaunchedEffect
            }
            val api = container.apiFor(url)
            val d = api.collectionDetail(collectionId)
            detail = d
            d.tmdbId?.let { id ->
                meta = runCatching { api.tmdbMetadata(id, d.kind) }.getOrNull()
            }
        } catch (e: Exception) {
            error = e.message ?: "Failed to load collection"
        }
    }

    val d = detail
    if (d == null) {
        Box(
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .padding(layout.gutterHorizontal),
            contentAlignment = Alignment.Center,
        ) {
            if (error != null) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(Spacing.md),
                ) {
                    Text(error!!, color = MaterialTheme.colorScheme.error)
                    Button(
                        onClick = onBack,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    ) { Text("Back") }
                }
            } else {
                Text("Loading collection…", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        return
    }

    LazyColumn(modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        item(key = "hero") {
            CollectionHero(detail = d, meta = meta, onBack = onBack)
        }

        if (d.kind == "tv" && d.episodes.isNotEmpty()) {
            // Group episodes by season for visual breaks. Each row
            // gets its own LazyColumn item so D-pad-down auto-scrolls
            // (BringIntoViewRequester not strictly needed here since
            // the list is plain Surface rows, but still).
            val grouped = d.episodes.groupBy { it.season }.toSortedMap()
            for ((season, episodes) in grouped) {
                item(key = "season-$season") {
                    Text(
                        "Season $season".uppercase(),
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(
                            horizontal = layout.gutterHorizontal,
                            vertical = Spacing.md,
                        ),
                    )
                }
                // Key on the physical-file identity `(infohash,
                // file_idx)` — the DB's UNIQUE constraint guarantees it
                // unique, so every file is its own reachable row even
                // when a mis-parsed pack collapses several leaves onto
                // the same (season, episode). Keying on (season,
                // episode) instead crashed the LazyColumn on duplicate
                // keys. Matches the web client (CollectionPage.tsx).
                // Secondary-sort by file_idx so collided rows keep a
                // stable order across refetches.
                items(
                    episodes.sortedWith(compareBy({ it.episode }, { it.fileIdx })),
                    key = { "${it.infohash}:${it.fileIdx}" },
                ) { ep ->
                    Box(
                        Modifier.padding(
                            horizontal = layout.gutterHorizontal,
                            vertical = Spacing.xs,
                        ),
                    ) {
                        EpisodeRow(
                            ep = ep,
                            onClick = { onPickFile(ep.infohash, ep.fileIdx) },
                        )
                    }
                }
            }
        } else {
            // Movie or unparsed-TV: list playable files across all
            // torrents in the collection, sorted by size descending.
            // Same fallback the web's /collection/:id uses.
            item(key = "files-header") {
                Text(
                    "Files".uppercase(),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(
                        horizontal = layout.gutterHorizontal,
                        vertical = Spacing.md,
                    ),
                )
            }
            val files: List<Pair<TorrentView, FileEntry>> = d.torrents.flatMap { t ->
                t.files
                    .filter { f -> VIDEO_EXTS_C.any { f.path.endsWith(it, ignoreCase = true) } }
                    .map { f -> t to f }
            }.sortedByDescending { (_, f) -> f.sizeBytes }
            items(files, key = { (t, f) -> "${t.infohash}:${f.index}" }) { (t, f) ->
                Box(
                    Modifier.padding(
                        horizontal = layout.gutterHorizontal,
                        vertical = Spacing.xs,
                    ),
                ) {
                    FileRow(
                        file = f,
                        onClick = { onPickFile(t.infohash, f.index) },
                    )
                }
            }
        }

        item(key = "trailing") {
            Box(Modifier.padding(vertical = Spacing.xl))
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun CollectionHero(
    detail: CollectionDetail,
    meta: TmdbMetadata?,
    onBack: () -> Unit,
) {
    val layout = LocalTvLayout.current
    val backdrop = tmdbBackdropUrl(meta?.backdropPath, "w1280")
    val poster = tmdbPosterUrl(meta?.posterPath, "w342")
    Box(Modifier.fillMaxWidth().aspectRatio(layout.heroAspect)) {
        if (backdrop != null) {
            AsyncImage(
                model = backdrop,
                contentDescription = detail.displayTitle,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
            Box(
                Modifier.fillMaxSize().background(
                    androidx.compose.ui.graphics.Brush.verticalGradient(
                        0.5f to androidx.compose.ui.graphics.Color.Transparent,
                        1f to androidx.compose.ui.graphics.Color.Black.copy(alpha = 0.85f),
                    ),
                ),
            )
        } else {
            Box(
                Modifier.fillMaxSize().background(
                    androidx.compose.ui.graphics.Brush.verticalGradient(
                        colors = listOf(
                            MaterialTheme.colorScheme.primary.copy(alpha = 0.30f),
                            androidx.compose.ui.graphics.Color(0xFF0B0D12),
                        ),
                    ),
                ),
            )
        }
        Row(
            Modifier
                .align(Alignment.BottomStart)
                .padding(
                    start = layout.gutterHorizontal,
                    end = layout.gutterHorizontal,
                    bottom = Spacing.lg,
                )
                .fillMaxWidth(),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(Spacing.xl),
        ) {
            if (poster != null) {
                AsyncImage(
                    model = poster,
                    contentDescription = null,
                    modifier = Modifier.width(120.dp).aspectRatio(2f / 3f),
                    contentScale = ContentScale.Crop,
                )
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Spacing.xs)) {
                Text(
                    detail.displayTitle,
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                val subtitle = buildString {
                    append(if (detail.kind == "tv") "Series" else "Movie")
                    append(" · ")
                    append(detail.torrents.size)
                    append(" torrent")
                    if (detail.torrents.size > 1) append("s")
                    if (detail.kind == "tv" && detail.episodes.isNotEmpty()) {
                        append(" · ${detail.episodes.size} episodes")
                    }
                }
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                meta?.overview?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 3,
                    )
                }
            }
            Button(
                onClick = onBack,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp),
            ) { Text("← Back") }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EpisodeRow(ep: CollectionEpisode, onClick: () -> Unit) {
    Card(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().height(64.dp),
        shape = CardDefaults.shape(shape = RoundedCornerShape(8.dp)),
        colors = CardDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
            contentColor = MaterialTheme.colorScheme.onSurface,
            focusedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
            focusedContentColor = MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Row(
            Modifier.fillMaxSize().padding(horizontal = Spacing.lg, vertical = Spacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
        ) {
            Text(
                "S%02dE%02d".format(ep.season, ep.episode),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.width(96.dp),
            )
            if (ep.watched) {
                Surface(
                    shape = RoundedCornerShape(4.dp),
                    colors = SurfaceDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color(0xFF6B7280).copy(alpha = 0.85f),
                    ),
                ) {
                    Text(
                        "watched",
                        style = MaterialTheme.typography.labelSmall,
                        color = androidx.compose.ui.graphics.Color.White,
                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                    )
                }
            }
            Box(Modifier.weight(1f))
            Text(
                if (ep.watched) "▶ Replay" else "▶ Play",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun FileRow(file: FileEntry, onClick: () -> Unit) {
    Card(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().height(64.dp),
        shape = CardDefaults.shape(shape = RoundedCornerShape(8.dp)),
        colors = CardDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
            contentColor = MaterialTheme.colorScheme.onSurface,
            focusedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
            focusedContentColor = MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Row(
            Modifier.fillMaxSize().padding(horizontal = Spacing.lg, vertical = Spacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    file.path.substringAfterLast('/'),
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                )
                Text(
                    formatFileSize(file.sizeBytes),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                "▶ Play",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

private fun formatFileSize(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0f MB", mb)
    return "$b B"
}
