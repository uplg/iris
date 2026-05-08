package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
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
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Button
import androidx.tv.material3.ButtonDefaults
import androidx.tv.material3.Card
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import coil.compose.AsyncImage
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.TmdbMetadata
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.tmdbPosterUrl

/**
 * Home screen with two horizontal shelves. Selecting a card jumps to
 * `Routes.WATCH` with the appropriate `(infohash, fileIdx)`.
 *
 * `loadVersion` is a coarse re-fetch trigger — bumping it re-runs the
 * LaunchedEffect, used by the Retry button when a fetch fails.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun HomeScreen(
    container: AppContainer,
    onPickTorrent: (infohash: String) -> Unit,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onOpenSettings: () -> Unit,
    onOpenSearch: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var continueWatching by remember { mutableStateOf<List<ContinueWatchingItem>>(emptyList()) }
    var torrents by remember { mutableStateOf<List<TorrentView>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(true) }
    var loadVersion by remember { mutableIntStateOf(0) }

    LaunchedEffect(loadVersion) {
        loading = true
        error = null
        try {
            val url = container.sessionStore.serverUrl.first()
            if (url == null) {
                error = "Not signed in"
                loading = false
                return@LaunchedEffect
            }
            val api: IrisApi = container.apiFor(url)
            // Run both in parallel — they're independent.
            val cw = runCatching { api.continueWatching() }
            val tor = runCatching { api.listTorrents() }
            // Surface the first failure as an error, but still display
            // whatever we did manage to fetch.
            continueWatching = cw.getOrDefault(emptyList())
            torrents = tor.getOrDefault(emptyList())
            val fail = listOfNotNull(cw.exceptionOrNull(), tor.exceptionOrNull()).firstOrNull()
            if (fail != null && torrents.isEmpty() && continueWatching.isEmpty()) {
                error = fail.message ?: "Failed to load library"
            }
        } catch (e: Exception) {
            error = e.message ?: "Failed to load library"
        } finally {
            loading = false
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .padding(40.dp),
        verticalArrangement = Arrangement.spacedBy(28.dp),
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                "Iris  /",
                style = MaterialTheme.typography.headlineLarge,
                color = MaterialTheme.colorScheme.primary,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                "Greek goddess of the rainbow",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        if (error != null) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(error!!, color = MaterialTheme.colorScheme.error)
                Button(
                    onClick = { loadVersion++ },
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                ) {
                    Text("Retry")
                }
            }
        } else if (loading && torrents.isEmpty() && continueWatching.isEmpty()) {
            Text(
                "Loading library…",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        if (continueWatching.isNotEmpty()) {
            Shelf(title = "Continue watching") {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    items(continueWatching, key = { "${it.infohash}:${it.fileIdx}" }) { item ->
                        ContinueWatchingCard(
                            container = container,
                            item = item,
                            onClick = { onPickFile(item.infohash, item.fileIdx) },
                        )
                    }
                }
            }
        }

        if (torrents.isNotEmpty()) {
            Shelf(title = "Library") {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    items(torrents, key = { it.infohash }) { t ->
                        TorrentCard(
                            container = container,
                            torrent = t,
                            // Single-file torrents go straight to play; everything
                            // else goes through DetailScreen so the user picks an
                            // episode (or just hits Play to use the largest file).
                            onClick = {
                                val videoCount = t.files.count { f ->
                                    VIDEO_EXTS.any { f.path.endsWith(it, ignoreCase = true) }
                                }
                                if (videoCount <= 1) {
                                    val idx = t.files
                                        .filter { f -> VIDEO_EXTS.any { f.path.endsWith(it, ignoreCase = true) } }
                                        .maxByOrNull { f -> f.sizeBytes }
                                        ?.index ?: 0
                                    onPickFile(t.infohash, idx)
                                } else {
                                    onPickTorrent(t.infohash)
                                }
                            },
                        )
                    }
                }
            }
        }

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp, Alignment.End),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Button(
                onClick = onOpenSearch,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
            ) {
                Text("Search")
            }
            Button(
                onClick = onOpenSettings,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
            ) {
                Text("Settings")
            }
        }
        // Suppress unused-variable warning for the coroutine scope; callbacks
        // outside of this function may want it later.
        @Suppress("UNUSED_EXPRESSION") scope
    }
}

private val VIDEO_EXTS = listOf(".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv")

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Shelf(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            title.uppercase(),
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        content()
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ContinueWatchingCard(
    container: AppContainer,
    item: ContinueWatchingItem,
    onClick: () -> Unit,
) {
    val title = item.filePath?.substringAfterLast('/') ?: item.torrentName
    val pct = item.durationSeconds?.takeIf { it > 0 }
        ?.let { ((item.positionSeconds / it) * 100).toInt().coerceIn(0, 100) } ?: 0
    PosterCard(
        container = container,
        tmdbId = item.tmdbId,
        title = title,
        subtitle = if (pct > 0) "$pct% watched" else "Just started",
        onClick = onClick,
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun TorrentCard(
    container: AppContainer,
    torrent: TorrentView,
    onClick: () -> Unit,
) {
    PosterCard(
        container = container,
        tmdbId = torrent.tmdbId,
        title = torrent.name ?: torrent.infohash.take(12),
        subtitle = torrent.state.replaceFirstChar { it.uppercase() },
        onClick = onClick,
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun PosterCard(
    container: AppContainer,
    tmdbId: Long?,
    title: String,
    subtitle: String,
    onClick: () -> Unit,
) {
    var meta by remember(tmdbId) { mutableStateOf<TmdbMetadata?>(null) }
    LaunchedEffect(tmdbId) {
        if (tmdbId == null) return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        meta = runCatching { container.apiFor(url).tmdbMetadata(tmdbId) }.getOrNull()
    }
    val posterUrl = tmdbPosterUrl(meta?.posterPath, "w342")

    Card(
        onClick = onClick,
        modifier = Modifier.width(180.dp),
        shape = CardDefaults.shape(shape = RoundedCornerShape(12.dp)),
    ) {
        Column {
            Box(
                Modifier
                    .fillMaxWidth()
                    .aspectRatio(2f / 3f),
                contentAlignment = Alignment.Center,
            ) {
                if (posterUrl != null) {
                    AsyncImage(
                        model = posterUrl,
                        contentDescription = title,
                        modifier = Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    Text(
                        text = title.take(2).uppercase(),
                        style = MaterialTheme.typography.headlineMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                )
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
