package studio.kahn.iris.tv.ui.screens

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
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.first
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.TmdbMetadata
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.tmdbPosterUrl

private val VIDEO_EXTS_DETAIL = listOf(
    ".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv",
)

/**
 * The "in between" screen for multi-file torrents (TV box sets, anime
 * seasons, multi-disc rips). Shows TMDB backdrop + overview, then the list of
 * playable video files with per-file progress badges. Picking a row jumps
 * straight to the WatchScreen.
 *
 * For single-file torrents [HomeScreen] short-circuits and goes directly to
 * WatchScreen — there's nothing meaningful to pick.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun DetailScreen(
    container: AppContainer,
    infohash: String,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onBack: () -> Unit,
) {
    var torrent by remember { mutableStateOf<TorrentView?>(null) }
    var meta by remember { mutableStateOf<TmdbMetadata?>(null) }
    var progresses by remember { mutableStateOf<List<FileProgressEntry>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(infohash) {
        try {
            val url = container.sessionStore.serverUrl.first()
                ?: run { error = "Not signed in"; return@LaunchedEffect }
            val api: IrisApi = container.apiFor(url)
            val t = api.getTorrent(infohash)
            torrent = t
            progresses = runCatching { api.torrentProgress(infohash) }.getOrDefault(emptyList())
            // Only call TMDB if the server has confirmed the mapping is
            // right (runtime ≈ probed duration). Unverified `tmdb_id`s
            // would happily point us at the wrong movie's poster /
            // synopsis, which is exactly the bug we're fixing.
            if (t.tmdbVerified) {
                t.tmdbId?.let { id ->
                    meta = runCatching { api.tmdbMetadata(id) }.getOrNull()
                }
            }
        } catch (e: Exception) {
            error = e.message ?: "Failed to load"
        }
    }

    val t = torrent
    if (t == null) {
        Box(Modifier.fillMaxSize().padding(40.dp), contentAlignment = Alignment.Center) {
            if (error != null) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(error!!, color = MaterialTheme.colorScheme.error)
                    Button(
                        onClick = onBack,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    ) { Text("Back") }
                }
            } else {
                Text("Loading…", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        return
    }

    val progressByIdx = remember(progresses) { progresses.associateBy { it.fileIdx } }
    val videoFiles = remember(t) {
        t.files.filter { f -> VIDEO_EXTS_DETAIL.any { f.path.endsWith(it, ignoreCase = true) } }
            .sortedBy { it.path }
    }

    Row(
        Modifier.fillMaxSize().padding(40.dp),
        horizontalArrangement = Arrangement.spacedBy(40.dp),
    ) {
        // Left rail: poster + metadata.
        Column(
            Modifier.width(320.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            val poster = tmdbPosterUrl(meta?.posterPath, "w500")
            Card(
                onClick = {},
                modifier = Modifier.fillMaxWidth().aspectRatio(2f / 3f),
                shape = CardDefaults.shape(shape = RoundedCornerShape(12.dp)),
            ) {
                val displayTitle = t.name ?: t.infohash.take(12)
                if (poster != null) {
                    AsyncImage(
                        model = poster,
                        contentDescription = displayTitle,
                        modifier = Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                        Text(
                            displayTitle.take(2).uppercase(),
                            style = MaterialTheme.typography.headlineLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            Text(
                // We *don't* substitute meta?.title here — TMDB resolution
                // can be wrong (same year, same family of names) and the
                // user gets a confidently-displayed title that mismatches
                // the file. Filename is the source of truth.
                t.name ?: t.infohash.take(12),
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
            )
            meta?.year?.let {
                Text("$it", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            meta?.overview?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 8,
                )
            }
            if (t.addedByName.isNotBlank()) {
                Text(
                    "Added by ${t.addedByName}" +
                        if (t.addedAt.length >= 10) " · ${t.addedAt.substring(0, 10)}" else "",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Button(
                onClick = onBack,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 10.dp),
            ) {
                Text("Back")
            }
        }

        // Right rail: file list.
        Column(
            Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                "Files (${videoFiles.size})".uppercase(),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                items(videoFiles, key = { it.index }) { f ->
                    FileRow(
                        file = f,
                        progress = progressByIdx[f.index],
                        onClick = { onPickFile(t.infohash, f.index) },
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun FileRow(
    file: FileEntry,
    progress: FileProgressEntry?,
    onClick: () -> Unit,
) {
    val name = file.path.substringAfterLast('/')
    val pct = progress?.durationSeconds?.takeIf { it > 0 }
        ?.let { ((progress.positionSeconds / it) * 100).toInt().coerceIn(0, 100) }
    val subtitle = when {
        progress?.completed == true -> "Watched"
        pct != null -> "$pct% watched"
        else -> formatBytes(file.sizeBytes)
    }
    Card(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().height(72.dp),
        shape = CardDefaults.shape(shape = RoundedCornerShape(8.dp)),
    ) {
        Row(
            Modifier.fillMaxSize().padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(name, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                if (progress?.completed == true) "✓" else "▶",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

private fun formatBytes(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format("%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format("%.0f MB", mb)
    return "$b B"
}
