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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
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
import studio.kahn.iris.tv.data.DescriptionFormat
import studio.kahn.iris.tv.data.IngestRequest
import studio.kahn.iris.tv.data.MediaInfoSummary
import studio.kahn.iris.tv.data.SubInfoDetails
import studio.kahn.iris.tv.data.TmdbMetadata
import studio.kahn.iris.tv.data.TorrentDetails
import studio.kahn.iris.tv.data.VideoInfoDetails
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.TvIconButton
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Spacing

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
                runCatching { api.tmdbMetadata(tmdbId, kind) }.getOrNull()
            } else null
            det to m
        }
        det.onSuccess { details = it }
        det.onFailure { error = it.message ?: "Failed to load details" }
        meta = m
        loading = false
    }

    val layout = LocalTvLayout.current

    // Park initial focus on the top-left Back (overlaid on the hero) so the
    // screen opens at the TOP with the title fully readable, and pressing ↑
    // from the Download action returns here. Auto-focusing the Download button
    // instead scrolled the title off-screen and trapped focus at the bottom
    // with nothing focusable above to scroll back up to.
    val backFocus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { backFocus.requestFocus() } }

    LazyColumn(modifier = Modifier.fillMaxSize()) {
        item(key = "hero") {
            Hero(
                meta = meta,
                tmdbId = tmdbId,
                fallbackTitle = details?.title ?: externalId,
                onBack = onBack,
                backFocus = backFocus,
                // Keep the hero to under half the viewport so the title +
                // Download action sit together on screen — no scrolling the
                // title out of view to reach the button.
                modifier = Modifier.fillParentMaxHeight(0.46f),
            )
        }

        item(key = "body") {
            Column(
                Modifier.padding(
                    horizontal = layout.gutterHorizontal,
                    vertical = Spacing.xxl,
                ),
                verticalArrangement = Arrangement.spacedBy(Spacing.xl),
            ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(
                    details?.title ?: "Loading…",
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.SemiBold,
                    // tv-material3 Text with no color falls back to a
                    // black LocalContentColor here (no enclosing Surface
                    // sets it) → black-on-dark. Every other Text on this
                    // screen sets it explicitly; the title was the one
                    // that didn't.
                    color = MaterialTheme.colorScheme.onSurface,
                )
                if (details?.freeleech == true) {
                    BadgeChip("Freeleech", color = IrisColors.Success)
                }
                if (details?.exclusive == true) {
                    BadgeChip("Exclusive", color = IrisColors.Warn)
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

            // Dead-torrent guard: a confirmed 0-seeder release can't be
            // grabbed (its pieces never fully assemble). The server blocks it
            // too; the UI just reflects that. 1 seeder is fine — no warning.
            val dead = details?.seeders == 0

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
                if (kind == "tv") {
                    IrisButton(
                        if (following) "Following…" else "♥  Follow",
                        {
                            if (following) return@IrisButton
                            val title = details?.title
                            if (title.isNullOrBlank()) return@IrisButton
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
                        variant = IrisButtonVariant.Ghost,
                        enabled = details != null && !following && !ingesting,
                    )
                }
                IrisButton(
                    if (dead) "Dead torrent" else if (ingesting) "Starting…" else "▶  Download & play",
                    {
                        if (ingesting || dead) return@IrisButton
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
                    enabled = details != null && !ingesting && !dead,
                )
            }

            if (loading) {
                Text("Reading details…", style = MaterialTheme.typography.bodyMedium)
            }
            error?.let {
                Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
            }

            // Synopsis: prefer TMDB (clean text) over the markup-laden
            // tracker description on TV. We strip whatever markup the
            // tracker uses (BBCode for torr9, HTML for c411) as a backup
            // so we never show raw tags on a 10-foot screen.
            val synopsis = meta?.overview ?: details?.let { d ->
                d.description?.let { desc ->
                    when (d.descriptionFormat) {
                        DescriptionFormat.BBCODE -> stripBBCode(desc)
                        DescriptionFormat.HTML -> stripHtml(desc)
                        DescriptionFormat.PLAIN -> desc
                    }
                }
            }
            if (!synopsis.isNullOrBlank()) {
                Text(
                    synopsis,
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.fillMaxWidth(0.8f),
                )
            }

            details?.mediaInfo?.let { FactsGrid(it) }
        }
        }

        item(key = "trailing") { Box(Modifier.padding(vertical = Spacing.xl)) }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Hero(
    meta: TmdbMetadata?,
    tmdbId: Long?,
    fallbackTitle: String,
    onBack: () -> Unit,
    backFocus: FocusRequester,
    modifier: Modifier = Modifier,
) {
    val backdrop = tmdbBackdropUrl(meta?.backdropPath, "w1280")
    val poster = tmdbPosterUrl(meta?.posterPath, "w342")
    val layout = LocalTvLayout.current
    Box(modifier.fillMaxWidth()) {
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
                                IrisColors.Brand.copy(alpha = 0.30f),
                                IrisColors.BackgroundDeep,
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
                    .padding(start = layout.gutterHorizontal, bottom = Spacing.xl)
                    .width(if (layout.gutterHorizontal >= 32.dp) 104.dp else 84.dp)
                    .aspectRatio(2f / 3f),
                contentScale = ContentScale.Crop,
            )
        }
        // Focusable Back at the top-left — holds initial focus so the screen
        // opens at the top (title readable) and ↑ from the actions returns here.
        TvIconButton(
            icon = Icons.AutoMirrored.Filled.ArrowBack,
            contentDescription = "Back",
            onClick = onBack,
            modifier = Modifier
                .align(Alignment.TopStart)
                .padding(start = layout.gutterHorizontal, top = Spacing.lg)
                .focusRequester(backFocus),
        )
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
        // HDR/DV: keep just the format name (the raw field can be
        // "Dolby Vision, Version 1.0, dvhe.08…") and render it as a normal
        // chip — the old amber accent looked garish on a 10-foot screen.
        v.hdr?.takeIf { it.isNotBlank() }?.let { ChipText(it.substringBefore(",").trim()) }
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
                    // No explicit color → black LocalContentColor on a
                    // dark screen. VideoFacts dodges this via ChipText;
                    // the plain Audio/Subtitles rows did not.
                    color = MaterialTheme.colorScheme.onSurface,
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
                    color = MaterialTheme.colorScheme.onSurface,
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
    val bg = if (accent) IrisColors.Warn else MaterialTheme.colorScheme.surfaceVariant
    Surface(
        shape = RoundedCornerShape(6.dp),
        colors = SurfaceDefaults.colors(containerColor = bg),
    ) {
        Text(
            text,
            style = MaterialTheme.typography.labelLarge,
            color = if (accent) IrisColors.OnBrand else MaterialTheme.colorScheme.onSurface,
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

/**
 * Minimal HTML-to-plain-text for the c411 description fallback. Strips
 * all tags, decodes the handful of entities indexers actually emit, and
 * collapses runs of whitespace. We do NOT try to preserve table layout
 * or images — TV synopsis is plain text only.
 */
private fun stripHtml(input: String): String {
    // Drop block-level tags as newlines so paragraphs don't collide.
    val withBreaks = input
        .replace(Regex("(?i)<(br|/p|/h[1-6]|/div|/li|/tr)\\b[^>]*>"), "\n")
    // Then drop all remaining tags.
    val noTags = withBreaks.replace(Regex("<[^>]+>"), "")
    // Decode the common entities. Anything else stays escaped — better
    // than a half-decoded mess.
    val decoded = noTags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
    // Collapse runs of whitespace, preserve paragraph breaks.
    return decoded
        .lineSequence()
        .map { it.replace(Regex("[ \\t]+"), " ").trim() }
        .joinToString("\n")
        .replace(Regex("\\n{3,}"), "\n\n")
        .trim()
}
