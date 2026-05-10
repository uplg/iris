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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
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
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.AddFollowRequest
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.AudioInfoDetails
import studio.kahn.iris.tv.data.IngestRequest
import studio.kahn.iris.tv.data.MediaInfoSummary
import studio.kahn.iris.tv.data.SubInfoDetails
import studio.kahn.iris.tv.data.TmdbMetadata
import studio.kahn.iris.tv.data.TorrentDetails
import studio.kahn.iris.tv.data.VideoInfoDetails
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.data.tmdbPosterUrl

/**
 * Full-screen detail view for a search hit. Shown when the user picks a
 * card on [SearchScreen]; lets them check what they're about to grab
 * before committing (audio/sub langs, video format, uploader, age, NFO
 * facts) and confirms with a big "Download" button. Modals are
 * awkward with a D-pad on TV — we push a screen instead.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun SearchDetailScreen(
    container: AppContainer,
    providerId: String,
    externalId: String,
    /** Optional TMDB id for the hero poster + backdrop. Caller passes
     *  this from the search result; if null, the hero falls back to a
     *  gradient placeholder. */
    tmdbId: Long?,
    /** `"tv"` or `"movie"` from the originating SearchResult. Drives
     *  whether the explicit "Follow series" action is rendered. */
    kind: String? = null,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onPickTorrent: (infohash: String) -> Unit,
    /** Invoked after a successful follow create — navigates to the
     *  newly-created Series page. Only fires when [kind] is `"tv"`. */
    onOpenSeries: (followId: String) -> Unit = {},
    onBack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var details by remember { mutableStateOf<TorrentDetails?>(null) }
    var meta by remember { mutableStateOf<TmdbMetadata?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(true) }
    var ingesting by remember { mutableStateOf(false) }
    var following by remember { mutableStateOf(false) }

    LaunchedEffect(providerId, externalId, tmdbId) {
        loading = true
        error = null
        val url = container.sessionStore.serverUrl.first()
        if (url == null) {
            error = "Not signed in"
            loading = false
            return@LaunchedEffect
        }
        val api = container.apiFor(url)
        val (det, m) = withContext(Dispatchers.IO) {
            val det = runCatching { api.torrentDetails(providerId, externalId) }
            val m = if (tmdbId != null) {
                runCatching { api.tmdbMetadata(tmdbId) }.getOrNull()
            } else null
            det to m
        }
        det.onSuccess { details = it }
        det.onFailure { error = it.message ?: "Failed to load details" }
        meta = m
        loading = false
    }

    val scroll = rememberScrollState()
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(scroll),
    ) {
        Hero(meta = meta, tmdbId = tmdbId, fallbackTitle = details?.title ?: externalId)

        Column(
            Modifier.padding(horizontal = 60.dp, vertical = 32.dp),
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(
                    details?.title ?: "Loading…",
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                if (details?.freeleech == true) {
                    BadgeChip("Freeleech", color = androidx.compose.ui.graphics.Color(0xFF10B981))
                }
                if (details?.exclusive == true) {
                    BadgeChip("Exclusive", color = androidx.compose.ui.graphics.Color(0xFFF59E0B))
                }
            }

            details?.let { d ->
                if (d.tags.isNotEmpty() || d.uploader != null || d.age != null) {
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        d.uploader?.let {
                            Text(
                                "Uploaded by $it${d.age?.let { age -> " · $age" } ?: ""}",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        if (d.tags.isNotEmpty()) {
                            Text(
                                d.tags.take(6).joinToString(" · "),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }

            if (loading) {
                Text("Reading details…", style = MaterialTheme.typography.bodyMedium)
            }
            error?.let {
                Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
            }

            // Synopsis: prefer TMDB (clean text) over the BBCode-laden
            // tracker description on TV. We strip the BBCode tags from
            // the tracker description as a backup so we never show raw
            // markup on a 10-foot screen.
            val synopsis = meta?.overview ?: details?.description?.let(::stripBBCode)
            if (!synopsis.isNullOrBlank()) {
                Text(
                    synopsis,
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.fillMaxWidth(0.8f),
                )
            }

            details?.mediaInfo?.let { FactsGrid(it) }

            // Stats line + action buttons.
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                details?.let { d ->
                    Text(
                        "↑ ${d.seeders ?: 0}   ↓ ${d.leechers ?: 0}" +
                            (d.fileSizeBytes?.let { "   ·   ${formatGiB(it)}" } ?: ""),
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Box(Modifier.weight(1f))
                Button(
                    onClick = onBack,
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 24.dp, vertical = 14.dp),
                ) { Text("Back") }
                if (kind == "tv") {
                    Button(
                        onClick = {
                            if (following) return@Button
                            val title = details?.title
                            if (title.isNullOrBlank()) return@Button
                            following = true
                            error = null
                            scope.launch {
                                try {
                                    val url = container.sessionStore.serverUrl.first()
                                        ?: return@launch run {
                                            error = "Not signed in"
                                            following = false
                                        }
                                    val api = container.apiFor(url)
                                    val created = withContext(Dispatchers.IO) {
                                        api.addFollow(
                                            AddFollowRequest(
                                                name = title,
                                                tmdbId = tmdbId,
                                            ),
                                        )
                                    }
                                    onOpenSeries(created.id)
                                } catch (e: Exception) {
                                    error = e.message ?: "Follow failed"
                                    following = false
                                }
                            }
                        },
                        enabled = details != null && !following && !ingesting,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 14.dp),
                    ) { Text(if (following) "Following…" else "♥  Follow") }
                }
                Button(
                    onClick = {
                        if (ingesting) return@Button
                        ingesting = true
                        error = null
                        scope.launch {
                            try {
                                val url = container.sessionStore.serverUrl.first()
                                    ?: return@launch run {
                                        error = "Not signed in"
                                        ingesting = false
                                    }
                                val api = container.apiFor(url)
                                val res = api.ingest(
                                    IngestRequest(
                                        providerId = providerId,
                                        externalId = externalId,
                                        tmdbId = tmdbId,
                                    )
                                )
                                val videoExts = listOf(
                                    ".mkv", ".mp4", ".webm", ".m4v", ".avi",
                                    ".mov", ".ts", ".mts", ".m2ts", ".wmv",
                                )
                                val videos = res.snapshot.files
                                    .filter { f -> videoExts.any { f.path.endsWith(it, ignoreCase = true) } }
                                if (videos.size <= 1) {
                                    val idx = videos.maxByOrNull { f -> f.sizeBytes }?.index ?: 0
                                    onPickFile(res.snapshot.infohash, idx)
                                } else {
                                    onPickTorrent(res.snapshot.infohash)
                                }
                            } catch (e: Exception) {
                                error = e.message ?: "Ingest failed"
                                ingesting = false
                            }
                        }
                    },
                    enabled = details != null && !ingesting,
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 28.dp, vertical = 14.dp),
                ) {
                    Text(if (ingesting) "Starting…" else "▶  Download & play")
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Hero(meta: TmdbMetadata?, tmdbId: Long?, fallbackTitle: String) {
    val backdrop = tmdbBackdropUrl(meta?.backdropPath, "w1280")
    val poster = tmdbPosterUrl(meta?.posterPath, "w342")
    Box(
        Modifier
            .fillMaxWidth()
            .aspectRatio(16f / 5f),
    ) {
        if (backdrop != null) {
            AsyncImage(
                model = backdrop,
                contentDescription = fallbackTitle,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
            // Dark gradient at the bottom so overlaid text stays legible.
            Box(
                Modifier
                    .fillMaxSize()
                    .background(
                        androidx.compose.ui.graphics.Brush.verticalGradient(
                            0.5f to androidx.compose.ui.graphics.Color.Transparent,
                            1f to androidx.compose.ui.graphics.Color.Black.copy(alpha = 0.85f),
                        ),
                    ),
            )
        } else {
            Box(
                Modifier
                    .fillMaxSize()
                    .background(
                        androidx.compose.ui.graphics.Brush.verticalGradient(
                            colors = listOf(
                                MaterialTheme.colorScheme.primary.copy(alpha = 0.30f),
                                androidx.compose.ui.graphics.Color(0xFF0B0D12),
                            ),
                        ),
                    ),
            )
        }
        if (poster != null) {
            AsyncImage(
                model = poster,
                contentDescription = null,
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .padding(start = 60.dp, bottom = 24.dp)
                    .width(120.dp)
                    .aspectRatio(2f / 3f),
                contentScale = ContentScale.Crop,
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun FactsGrid(mi: MediaInfoSummary) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        mi.video?.let { VideoFacts(it) }
        if (mi.audio.isNotEmpty()) AudioFacts(mi.audio)
        if (mi.subtitles.isNotEmpty()) SubFacts(mi.subtitles)
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun VideoFacts(v: VideoInfoDetails) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        SectionLabel("Video")
        v.codec?.let { ChipText(it) }
        v.resolution?.let { ChipText(it) }
        v.fps?.let { ChipText("${"%.2f".format(it)} fps") }
        v.bitrateKbps?.let { ChipText("${it.formatThousands()} kb/s") }
        v.hdr?.let { ChipText(it, accent = true) }
        v.durationSecs?.let {
            Text(
                "· ${formatRuntime(it)}",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun AudioFacts(audio: List<AudioInfoDetails>) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
        SectionLabel("Audio")
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            audio.forEach { a ->
                Text(
                    listOfNotNull(
                        a.lang ?: "?",
                        a.commercialName ?: a.codec,
                        a.channels?.let { channelLabel(it) },
                        a.bitrateKbps?.let { "${it} kb/s" },
                        if (a.default) "default" else null,
                    ).joinToString(" · "),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SubFacts(subs: List<SubInfoDetails>) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
        SectionLabel("Subtitles")
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            subs.forEach { s ->
                Text(
                    listOfNotNull(
                        s.lang ?: "?",
                        s.format,
                        if (s.forced) "forced" else null,
                        if (s.title?.contains("SDH", ignoreCase = true) == true) "SDH" else null,
                    ).joinToString(" · "),
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SectionLabel(label: String) {
    Text(
        label.uppercase(),
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.width(120.dp).padding(top = 2.dp),
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ChipText(text: String, accent: Boolean = false) {
    val bg = if (accent) {
        androidx.compose.ui.graphics.Color(0xFFF59E0B)
    } else {
        MaterialTheme.colorScheme.surfaceVariant
    }
    Surface(
        shape = RoundedCornerShape(6.dp),
        colors = SurfaceDefaults.colors(containerColor = bg),
    ) {
        Text(
            text,
            style = MaterialTheme.typography.labelLarge,
            color = if (accent) androidx.compose.ui.graphics.Color.Black else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp),
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun BadgeChip(text: String, color: androidx.compose.ui.graphics.Color) {
    Surface(
        shape = RoundedCornerShape(4.dp),
        colors = SurfaceDefaults.colors(containerColor = color),
    ) {
        Text(
            text,
            style = MaterialTheme.typography.labelSmall,
            color = androidx.compose.ui.graphics.Color.White,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
        )
    }
}

private fun channelLabel(n: Int): String = when (n) {
    1 -> "1.0"
    2 -> "2.0"
    6 -> "5.1"
    8 -> "7.1"
    else -> "${n}ch"
}

private fun formatRuntime(secs: Int): String {
    val h = secs / 3600
    val m = (secs % 3600) / 60
    return if (h > 0) "${h}h${m.toString().padStart(2, '0')}" else "${m}min"
}

private fun formatGiB(b: Long): String {
    val gib = b.toDouble() / (1024.0 * 1024.0 * 1024.0)
    return "%.2f GiB".format(gib)
}

private fun Int.formatThousands(): String =
    "%,d".format(this).replace(',', ' ')

/**
 * Minimal BBCode stripper for the synopsis fallback. We don't try to
 * render colours / images on TV — too noisy on a 10-foot UI — just yank
 * the markup so the raw text is readable. Used only when TMDB doesn't
 * have a synopsis (very rare; nearly every torr9 listing has one).
 */
private fun stripBBCode(input: String): String {
    var out = input.replace(Regex("\\[/?[a-zA-Z]+(=[^\\]]+)?]"), "")
    // Drop pure-decoration lines.
    out = out.lineSequence()
        .filter { line ->
            val trimmed = line.trim()
            if (trimmed.isEmpty()) return@filter true
            val deco = trimmed.count { it in "━—–·•⋯" }
            deco.toDouble() / trimmed.length < 0.5
        }
        .joinToString("\n")
        .trim()
    return out
}
