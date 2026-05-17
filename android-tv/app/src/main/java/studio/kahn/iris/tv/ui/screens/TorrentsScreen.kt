package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Button
import androidx.tv.material3.ButtonDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.Text
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.LibraryResponse
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Spacing

private val VIDEO_EXTS = listOf(
    ".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv",
)

/**
 * Seedbox / raw-torrents management view. Mirrors the web's
 * `?view=torrents`: every ingested torrent with its live transfer
 * stats, a per-torrent delete (the backend also wipes the files from
 * disk + reclaims space), and direct playback of the contained video
 * files. The family-friendly Library shelf on Home is untouched — this
 * is the "it's actually a seedbox" surface, reached from the Home top
 * bar.
 *
 * Identity: each torrent is keyed on its `infohash` (unique). Lifetime
 * upload + the seed summary come from the same `?view=torrents`
 * payload the web header uses, so the "since the beginning" total
 * survives GC eviction of individual torrents.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun TorrentsScreen(
    container: AppContainer,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onBack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val layout = LocalTvLayout.current
    var torrents by remember { mutableStateOf<List<TorrentView>?>(null) }
    var totalUploaded by remember { mutableStateOf(0L) }
    var error by remember { mutableStateOf<String?>(null) }
    var loadVersion by remember { mutableIntStateOf(0) }
    // infohash currently being deleted — disables that row's actions
    // and survives across the 5 s refresh (keyed list state).
    var deleting by remember { mutableStateOf<String?>(null) }

    suspend fun fetch(): Pair<List<TorrentView>, Long>? {
        val url = container.sessionStore.serverUrl.first() ?: return null
        val res = withContext(Dispatchers.IO) {
            runCatching { container.apiFor(url).library("torrents") }.getOrNull()
        }
        val t = res as? LibraryResponse.Torrents ?: return null
        return t.items to t.totalUploadedBytes
    }

    LaunchedEffect(loadVersion) {
        error = null
        val url = container.sessionStore.serverUrl.first()
        if (url == null) {
            error = "Not signed in"
            return@LaunchedEffect
        }
        val got = runCatching { fetch() }.getOrNull()
        if (got == null) {
            if (torrents == null) error = "Failed to load torrents"
        } else {
            torrents = got.first
            totalUploaded = got.second
        }
    }

    // Live refresh so progress / speeds tick, same idiom as HomeScreen's
    // downloading shelf. Only reassign when the payload actually changed
    // so focused rows don't recompose mid-interaction.
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(5_000)
            val got = runCatching { fetch() }.getOrNull() ?: continue
            if (got.first != torrents) torrents = got.first
            if (got.second != totalUploaded) totalUploaded = got.second
        }
    }

    val onDelete: (String) -> Unit = onDelete@{ infohash ->
        if (deleting != null) return@onDelete
        deleting = infohash
        scope.launch {
            val url = container.sessionStore.serverUrl.first()
            if (url == null) {
                error = "Not signed in"
                deleting = null
                return@launch
            }
            try {
                withContext(Dispatchers.IO) { container.apiFor(url).deleteTorrent(infohash) }
                // Optimistic drop so the row vanishes immediately; the
                // refresh loop reconciles the rest.
                torrents = torrents?.filterNot { it.infohash == infohash }
            } catch (e: Exception) {
                error = e.message ?: "Delete failed"
            } finally {
                deleting = null
            }
        }
    }

    val list = torrents
    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
        contentPadding = PaddingValues(
            horizontal = layout.gutterHorizontal,
            vertical = layout.gutterVertical,
        ),
        verticalArrangement = Arrangement.spacedBy(Spacing.md),
    ) {
        item(key = "header") {
            Column(verticalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Column {
                        Text(
                            "Seedbox",
                            style = MaterialTheme.typography.headlineMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        Text(
                            if (list == null) "Loading torrents…"
                            else "${list.size} torrent${if (list.size == 1) "" else "s"}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Button(
                        onClick = onBack,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
                    ) { Text("← Back") }
                }
                if (list != null) SeedSummary(list, totalUploaded)
                error?.let {
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
            }
        }

        if (list != null && list.isEmpty()) {
            item(key = "empty") {
                Text(
                    "No torrents. Add content from Search.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(vertical = Spacing.lg),
                )
            }
        }

        items(list.orEmpty(), key = { it.infohash }) { t ->
            TorrentRow(
                t = t,
                deleting = deleting == t.infohash,
                onPlayFile = onPickFile,
                onDelete = { onDelete(t.infohash) },
            )
        }

        item(key = "trailing") { Box(Modifier.padding(vertical = Spacing.xl)) }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SeedSummary(items: List<TorrentView>, totalUploaded: Long) {
    val downBps = items.sumOf { it.downloadSpeedBps }
    val upBps = items.sumOf { it.uploadSpeedBps }
    val downloaded = items.sumOf { it.progressBytes }
    val ratio = if (downloaded > 0) totalUploaded.toDouble() / downloaded else null
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        colors = SurfaceDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.35f),
        ),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = Spacing.lg, vertical = Spacing.md),
            horizontalArrangement = Arrangement.spacedBy(Spacing.xl),
        ) {
            Stat("Seeded all-time", formatBytes(totalUploaded))
            Stat("Ratio", ratio?.let { "%.2f".format(it) } ?: "—")
            Stat("Live", "↓ ${formatBytes(downBps)}/s   ↑ ${formatBytes(upBps)}/s")
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Stat(label: String, value: String) {
    Column {
        Text(
            label.uppercase(),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(value, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold)
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun TorrentRow(
    t: TorrentView,
    deleting: Boolean,
    onPlayFile: (infohash: String, fileIdx: Int) -> Unit,
    onDelete: () -> Unit,
) {
    // Per-row UI state; survives the 5 s refresh because the LazyColumn
    // item is keyed on infohash.
    var expanded by remember(t.infohash) { mutableStateOf(false) }
    var confirming by remember(t.infohash) { mutableStateOf(false) }

    val videos = remember(t.files) {
        t.files
            .filter { f -> VIDEO_EXTS.any { f.path.endsWith(it, ignoreCase = true) } }
            .sortedByDescending { it.sizeBytes }
    }
    val pct = t.progressPct.coerceIn(0f, 100f)
    val finished = pct >= 100f
    val ratio = if (t.progressBytes > 0) {
        t.uploadedBytesTotal.toDouble() / t.progressBytes
    } else {
        null
    }

    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = SurfaceDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
        ),
    ) {
        Column(
            Modifier.fillMaxWidth().padding(Spacing.lg),
            verticalArrangement = Arrangement.spacedBy(Spacing.sm),
        ) {
            Text(
                t.name ?: t.infohash,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
            )
            Row(
                horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                StateBadge(t.state)
                Text(
                    "${t.peers} peer${if (t.peers == 1) "" else "s"}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                ratio?.let {
                    Text(
                        "ratio ${"%.2f".format(it)}",
                        style = MaterialTheme.typography.labelSmall,
                        color = if (it >= 1.0) {
                            Color(0xFF10B981)
                        } else {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        },
                    )
                }
            }

            ProgressBar(pct = pct, finished = finished)
            Text(
                "${formatBytes(t.progressBytes)} / ${formatBytes(t.totalSizeBytes)}" +
                    "   ·   ↓ ${formatBytes(t.downloadSpeedBps)}/s" +
                    "   ↑ ${formatBytes(t.uploadSpeedBps)}/s",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "Added by ${t.addedByName.ifBlank { "—" }}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            t.error?.let {
                Text(it, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error)
            }

            Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                if (confirming) {
                    Button(
                        onClick = onDelete,
                        enabled = !deleting,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        colors = ButtonDefaults.colors(
                            containerColor = Color(0xFFB91C1C),
                            contentColor = Color.White,
                        ),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
                    ) { Text(if (deleting) "Deleting…" else "Confirm delete") }
                    Button(
                        onClick = { confirming = false },
                        enabled = !deleting,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
                    ) { Text("Cancel") }
                } else {
                    if (videos.size == 1) {
                        Button(
                            onClick = { onPlayFile(t.infohash, videos[0].index) },
                            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                            contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                        ) { Text("▶ Play") }
                    }
                    if (videos.size > 1) {
                        Button(
                            onClick = { expanded = !expanded },
                            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                            contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
                        ) { Text(if (expanded) "Hide files" else "${videos.size} files") }
                    }
                    Button(
                        onClick = { confirming = true },
                        enabled = !deleting,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
                    ) { Text("Delete") }
                }
            }

            if (expanded && videos.size > 1) {
                Column(
                    Modifier.fillMaxWidth().padding(top = Spacing.xs),
                    verticalArrangement = Arrangement.spacedBy(Spacing.xs),
                ) {
                    for (f in videos) {
                        Button(
                            onClick = { onPlayFile(t.infohash, f.index) },
                            modifier = Modifier.fillMaxWidth(),
                            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                            colors = ButtonDefaults.colors(
                                containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f),
                                contentColor = MaterialTheme.colorScheme.onSurface,
                            ),
                            contentPadding = PaddingValues(horizontal = Spacing.lg, vertical = Spacing.md),
                        ) {
                            Row(
                                Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    f.path.substringAfterLast('/'),
                                    style = MaterialTheme.typography.bodySmall,
                                    maxLines = 1,
                                )
                                Text(
                                    "▶ ${formatBytes(f.sizeBytes)}",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.primary,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ProgressBar(pct: Float, finished: Boolean) {
    val track = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.25f)
    val fill = if (finished) Color(0xFF10B981) else MaterialTheme.colorScheme.primary
    Box(
        Modifier
            .fillMaxWidth()
            .height(6.dp)
            .background(track, RoundedCornerShape(3.dp)),
    ) {
        Box(
            Modifier
                .fillMaxWidth(fraction = (pct / 100f).coerceIn(0f, 1f))
                .height(6.dp)
                .background(fill, RoundedCornerShape(3.dp)),
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun StateBadge(state: String) {
    val (label, color) = when (state) {
        "live" -> "live" to Color(0xFF10B981)
        "paused" -> "paused" to Color(0xFFF59E0B)
        "error" -> "error" to Color(0xFFEF4444)
        else -> "initializing" to Color(0xFF6B7280)
    }
    Surface(
        shape = RoundedCornerShape(4.dp),
        colors = SurfaceDefaults.colors(containerColor = color.copy(alpha = 0.85f)),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = Color.White,
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
        )
    }
}

private fun formatBytes(b: Long): String {
    if (b < 1_000) return "$b B"
    val units = listOf("KB", "MB", "GB", "TB")
    var v = b.toDouble() / 1_000.0
    var i = 0
    while (v >= 1_000.0 && i < units.size - 1) {
        v /= 1_000.0
        i++
    }
    return if (v >= 100) "%.0f %s".format(v, units[i]) else "%.1f %s".format(v, units[i])
}
