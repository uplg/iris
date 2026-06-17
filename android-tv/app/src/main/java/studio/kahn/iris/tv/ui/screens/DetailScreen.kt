package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.tv.material3.Border
import androidx.tv.material3.Card
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.first
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.FileProgressEntry
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.components.Chip
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.irisPosterPlaceholder
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing
import studio.kahn.iris.tv.ui.theme.irisAmbient

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
    var meta by remember { mutableStateOf<MediaMetadata?>(null) }
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
            // Trust the server's tmdb_id without the verified gate —
            // the lookup endpoint already does a kind-namespace
            // fallback (`/movie/X` → `/tv/X`), so even a wrong-kind
            // hint resolves to *something*. Hiding the poster on
            // every unverified torrent was hurting UX more than the
            // rare wrong-poster mismatch.
            t.tmdbId?.let { id ->
                meta = runCatching { api.tmdbMetadata(id, t.kind?.value) }.getOrNull()
            }
        } catch (e: Exception) {
            error = e.message ?: "Failed to load"
        }
    }

    val layout = LocalTvLayout.current
    val t = torrent
    if (t == null) {
        Box(
            Modifier
                .fillMaxSize()
                .background(IrisColors.Background)
                .padding(layout.gutterHorizontal),
            contentAlignment = Alignment.Center,
        ) {
            if (error != null) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    Text(error!!, color = MaterialTheme.colorScheme.error)
                    IrisButton("Back", onBack, variant = IrisButtonVariant.Ghost, icon = Icons.AutoMirrored.Filled.ArrowBack)
                }
            } else {
                Text("Loading…", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        return
    }

    val progressByIdx = remember(progresses) { progresses.associateBy { it.fileIdx.toInt() } }
    val videoFiles = remember(t) {
        t.files.filter { f -> VIDEO_EXTS_DETAIL.any { f.path.endsWith(it, ignoreCase = true) } }
            .sortedBy { it.path }
    }

    Box(Modifier.fillMaxSize().background(IrisColors.Background)) {
        // Ambient backlight wash behind the detail, matching Home (web `.ambient`).
        Box(Modifier.fillMaxSize().background(irisAmbient()))
        Row(
            Modifier
                .fillMaxSize()
                .padding(
                    horizontal = layout.gutterHorizontal,
                    vertical = layout.gutterVertical,
                ),
            horizontalArrangement = Arrangement.spacedBy(Spacing.xxl),
        ) {
            // Left rail: poster + metadata.
            Column(
                Modifier.width(layout.detailRail),
                verticalArrangement = Arrangement.spacedBy(Spacing.lg),
            ) {
                val poster = tmdbPosterUrl(meta?.posterPath, "w500")
                val posterShape = RoundedCornerShape(Radius.poster)
                // Non-clickable Surface — the poster has no useful tap
                // action (the previous `onClick = {}` was a placeholder)
                // but Card's default focus scale (~1.1×) was zooming
                // the already-2:3 poster off-screen.
                Surface(
                    modifier = Modifier.fillMaxWidth().aspectRatio(2f / 3f),
                    shape = posterShape,
                    border = Border(BorderStroke(1.dp, IrisColors.BorderStrong), shape = posterShape),
                    colors = androidx.tv.material3.SurfaceDefaults.colors(containerColor = IrisColors.Card),
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
                        Box(Modifier.fillMaxSize().background(irisPosterPlaceholder()))
                        Box(
                            Modifier.fillMaxSize().padding(16.dp),
                            contentAlignment = Alignment.BottomStart,
                        ) {
                            Text(
                                displayTitle,
                                style = MaterialTheme.typography.headlineSmall,
                                color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.92f),
                                maxLines = 4,
                            )
                        }
                    }
                }
                Eyebrow(if (t.kind == MediaKind.tv) "Series" else "Movie", color = IrisColors.Brand)
                Text(
                    // We *don't* substitute meta?.title here — TMDB resolution
                    // can be wrong (same year, same family of names) and the
                    // user gets a confidently-displayed title that mismatches
                    // the file. Filename is the source of truth.
                    t.name ?: t.infohash.take(12),
                    style = MaterialTheme.typography.headlineSmall,
                )
                meta?.year?.let {
                    Chip("$it")
                }
                meta?.overview?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 8,
                    )
                }
                if (t.addedByName.isNotBlank()) {
                    Text(
                        "Added by ${t.addedByName}" +
                            if (t.addedAt.toString().length >= 10) " · ${t.addedAt.toString().substring(0, 10)}" else "",
                        style = MaterialTheme.typography.labelSmall,
                        color = IrisColors.FgDim,
                    )
                }
                IrisButton("Back", onBack, variant = IrisButtonVariant.Ghost, icon = Icons.AutoMirrored.Filled.ArrowBack)
            }

            // Right rail: file list.
            Column(
                Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Eyebrow("Files · ${videoFiles.size}")
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
    // Keep the focused colors inside the dark palette. The default
    // `CardDefaults.colors()` inverts to `inverseSurface` /
    // `inverseOnSurface` on focus, which renders as a bright card
    // with BLACK text on our dark theme — illegible. Focus instead
    // reads as the shared design-system brand ring + glow.
    val rowShape = RoundedCornerShape(Radius.button)
    Card(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().height(72.dp),
        shape = CardDefaults.shape(shape = rowShape),
        scale = CardDefaults.scale(focusedScale = 1f),
        colors = CardDefaults.colors(
            containerColor = IrisColors.Overlay06,
            contentColor = MaterialTheme.colorScheme.onSurface,
            focusedContainerColor = IrisColors.Overlay12,
            focusedContentColor = MaterialTheme.colorScheme.onSurface,
        ),
        border = CardDefaults.border(
            focusedBorder = Border(BorderStroke(Focus.ring, IrisColors.Brand), shape = rowShape),
        ),
    ) {
        Row(
            Modifier.fillMaxSize().padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    name,
                    style = MaterialTheme.typography.bodyLarge,
                    maxLines = 1,
                    color = MaterialTheme.colorScheme.onSurface,
                )
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
    if (gb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0f MB", mb)
    return "$b B"
}
