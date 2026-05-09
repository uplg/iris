package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.fillMaxWidth
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.foundation.layout.height
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
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.FeaturedResponse
import studio.kahn.iris.tv.data.FollowSummary
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.LibraryResponse
import studio.kahn.iris.tv.data.SearchResult
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
    /** Open the search screen. When `query` is non-null the search runs
     *  immediately with that string pre-filled — used by Featured Movies
     *  cards (which don't have a dedicated detail page yet). */
    onOpenSearch: (query: String?) -> Unit,
    /** Open the SeriesScreen for a TV show. Used by Watchlist cards and
     *  by Featured Series cards (TMDB id available, no torrent yet). */
    onOpenSeries: (tmdbId: Long) -> Unit,
) {
    val scope = rememberCoroutineScope()
    var continueWatching by remember { mutableStateOf<List<ContinueWatchingItem>>(emptyList()) }
    // Two separate states for the two shelves so a tick that only touches
    // a Downloading entry's progress/speed doesn't invalidate Library —
    // those cards stay frozen and skip recomposition entirely.
    var downloading by remember { mutableStateOf<List<TorrentView>>(emptyList()) }
    var library by remember { mutableStateOf<List<TorrentView>>(emptyList()) }
    var watchlist by remember { mutableStateOf<List<FollowSummary>>(emptyList()) }
    var featured by remember { mutableStateOf<FeaturedResponse?>(null) }
    var collections by remember { mutableStateOf<List<CollectionListItem>>(emptyList()) }
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
            // Both calls + the JSON parse run on Dispatchers.IO so the main
            // thread stays free to render the UI even on a slow tunnel.
            data class HomeFetch(
                val cw: Result<List<ContinueWatchingItem>>,
                val tor: Result<List<TorrentView>>,
                val follows: Result<List<FollowSummary>>,
                val featured: Result<FeaturedResponse>,
                val collections: Result<LibraryResponse>,
            )
            val fetch = withContext(Dispatchers.IO) {
                // Discovery / watchlist / library failures shouldn't
                // break the rest of the home — surface as empty
                // shelves, not as the global error banner.
                HomeFetch(
                    cw = runCatching { api.continueWatching() },
                    tor = runCatching { api.listTorrents() },
                    follows = runCatching { api.listFollows() },
                    featured = runCatching { api.discoverFeatured() },
                    collections = runCatching { api.library("collections") },
                )
            }
            val cw = fetch.cw
            val tor = fetch.tor
            val follows = fetch.follows
            val feat = fetch.featured
            val coll = fetch.collections
            continueWatching = cw.getOrDefault(emptyList())
            val fresh = tor.getOrDefault(emptyList())
            val (newDl, newLib) = splitTorrents(fresh)
            downloading = newDl
            library = newLib
            watchlist = follows.getOrDefault(emptyList())
            featured = feat.getOrNull()
            collections = (coll.getOrNull() as? LibraryResponse.Collections)?.items.orEmpty()
            val fail = listOfNotNull(cw.exceptionOrNull(), tor.exceptionOrNull()).firstOrNull()
            if (fail != null && fresh.isEmpty() && continueWatching.isEmpty()) {
                error = fail.message ?: "Failed to load library"
            }
        } catch (e: Exception) {
            error = e.message ?: "Failed to load library"
        } finally {
            loading = false
        }
    }

    // Re-fetch the live torrent state every 5s so the "Downloading" shelf's
    // progress bars and speeds tick up without the user mashing Retry.
    //
    // The whole network round-trip + JSON parse + filter/sort runs on
    // Dispatchers.IO, then we hop back to the composition thread and only
    // assign the *one* state slice that actually changed. Result: Library
    // cards never see a state-change event when only Downloading items
    // moved, so Compose smart-skips them entirely.
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(5_000)
            val url = container.sessionStore.serverUrl.first() ?: continue
            val split = withContext(Dispatchers.IO) {
                runCatching { container.apiFor(url).listTorrents() }
                    .map(::splitTorrents)
                    .getOrNull()
            } ?: continue
            val (newDl, newLib) = split
            // Per-shelf diff: Library only re-emits when the *Library* subset
            // changed (a torrent finished downloading, or was deleted, or
            // ingested). Same for Downloading.
            if (newDl != downloading) downloading = newDl
            if (newLib != library) library = newLib
        }
    }

    // The whole screen has to scroll vertically — three shelves + a top bar
    // overflow a 1080p TV when we list more than a handful of items each.
    // Without `verticalScroll` D-pad focus can't reach the lower shelves
    // because they're rendered off-canvas.
    val scrollState = rememberScrollState()
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(scrollState)
            .padding(40.dp),
        verticalArrangement = Arrangement.spacedBy(28.dp),
    ) {
        // Brand on the left, action chips (Search / Settings) pinned to the
        // right edge — TV remotes lose any button that lives at the bottom of
        // a vertical list once the focus drops into the shelves below.
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                "Iris  /",
                style = MaterialTheme.typography.headlineLarge,
                color = MaterialTheme.colorScheme.primary,
                fontWeight = FontWeight.SemiBold,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Button(
                    onClick = { onOpenSearch(null) },
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                ) { Text("🔍  Search") }
                Button(
                    onClick = onOpenSettings,
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                ) { Text("⚙  Settings") }
            }
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
        } else if (loading && downloading.isEmpty() && library.isEmpty() && continueWatching.isEmpty()) {
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

        if (watchlist.isNotEmpty()) {
            Shelf(title = "Ma Watchlist · ${watchlist.size}") {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    items(watchlist, key = { it.tmdbId }) { f ->
                        WatchlistCard(
                            container = container,
                            follow = f,
                            onClick = { onOpenSeries(f.tmdbId) },
                        )
                    }
                }
            }
        }

        featured?.let { f ->
            if (f.movies.isNotEmpty()) {
                Shelf(title = "Sorties Ciné · ${f.movies.size}") {
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        items(f.movies, key = { "${it.providerId}:${it.externalId}" }) { r ->
                            FeaturedCard(
                                container = container,
                                result = r,
                                onClick = { onOpenSearch(r.title) },
                            )
                        }
                    }
                }
            }
            if (f.series.isNotEmpty()) {
                Shelf(title = "Sorties Séries · ${f.series.size}") {
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        items(f.series, key = { "${it.providerId}:${it.externalId}" }) { r ->
                            FeaturedCard(
                                container = container,
                                result = r,
                                // TV shows with a TMDB id route straight
                                // to the series page (Follow + episode
                                // browse). Anything without falls back
                                // to a pre-filled search.
                                onClick = {
                                    val tid = r.tmdbId
                                    if (tid != null) onOpenSeries(tid)
                                    else onOpenSearch(r.title)
                                },
                            )
                        }
                    }
                }
            }
        }

        if (downloading.isNotEmpty()) {
            Shelf(title = "Downloading · ${downloading.size}") {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    items(downloading, key = { it.infohash }) { t ->
                        DownloadingCard(
                            container = container,
                            torrent = t,
                            onClick = { routeTorrent(t, onPickFile, onPickTorrent) },
                        )
                    }
                }
            }
        }

        if (collections.isNotEmpty()) {
            Shelf(title = "Bibliothèque · ${collections.size}") {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    items(collections, key = { it.id }) { c ->
                        CollectionCard(
                            container = container,
                            collection = c,
                            onClick = {
                                // Routing: TV with TMDB → Series page,
                                // movie / fallback → /watch on the
                                // representative torrent's first file.
                                // (Picking the right file index is handled
                                // by routeTorrent on the snapshot lookup.)
                                if (c.kind == "tv" && c.tmdbId != null) {
                                    onOpenSeries(c.tmdbId)
                                } else if (c.representativeInfohash != null) {
                                    val snap = library.firstOrNull {
                                        it.infohash == c.representativeInfohash
                                    }
                                    if (snap != null) {
                                        routeTorrent(snap, onPickFile, onPickTorrent)
                                    } else {
                                        // Engine snapshot not loaded yet —
                                        // fall back to the detail screen
                                        // which polls for it.
                                        onPickTorrent(c.representativeInfohash)
                                    }
                                }
                            },
                        )
                    }
                }
            }
        }

        // The coroutine scope outlives the recomposition; we may want to
        // wire it to background tasks (poster prefetch, etc.) later.
        @Suppress("UNUSED_EXPRESSION") scope
    }
}

private val VIDEO_EXTS = listOf(".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv")

/**
 * Partition the raw torrent list into the two shelves the Home screen
 * renders: still-downloading (with progress < 100%) vs library
 * (fully fetched). Anything in error or paused at < 100% stays in
 * Downloading so the user can see what's broken.
 */
private fun splitTorrents(all: List<TorrentView>): Pair<List<TorrentView>, List<TorrentView>> {
    val dl = mutableListOf<TorrentView>()
    val lib = mutableListOf<TorrentView>()
    for (t in all) {
        if (t.progressPct >= 99.9f) lib.add(t) else dl.add(t)
    }
    dl.sortByDescending { it.progressPct }
    lib.sortBy { (it.name ?: it.infohash).lowercase() }
    return dl to lib
}

/**
 * Pick the right destination when the user hits a Library/Downloading
 * card: single-video torrents go straight to play (largest video), every-
 * thing else lands on DetailScreen for episode selection.
 */
private fun routeTorrent(
    t: TorrentView,
    onPickFile: (String, Int) -> Unit,
    onPickTorrent: (String) -> Unit,
) {
    val videos = t.files.filter { f ->
        VIDEO_EXTS.any { f.path.endsWith(it, ignoreCase = true) }
    }
    if (videos.size <= 1) {
        val idx = videos.maxByOrNull { f -> f.sizeBytes }?.index ?: 0
        onPickFile(t.infohash, idx)
    } else {
        onPickTorrent(t.infohash)
    }
}

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
    val rawTitle = item.filePath?.substringAfterLast('/') ?: item.torrentName
    val pct = item.durationSeconds?.takeIf { it > 0 }
        ?.let { ((item.positionSeconds / it).toFloat()).coerceIn(0f, 1f) }
    PosterCard(
        container = container,
        tmdbId = item.tmdbId,
        tmdbVerified = item.tmdbVerified,
        title = prettifyFilename(rawTitle),
        subtitle = pct?.let { "${(it * 100).toInt()}% watched" } ?: "Just started",
        progress = pct,
        progressColor = null, // primary = watch progress
        onClick = onClick,
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun WatchlistCard(
    container: AppContainer,
    follow: FollowSummary,
    onClick: () -> Unit,
) {
    val subtitle = follow.totalSeasons?.let {
        "$it saison${if (it > 1) "s" else ""}"
    } ?: "Suivi"
    PosterCard(
        container = container,
        tmdbId = follow.tmdbId,
        // FollowSummary's poster_path comes from the same TMDB lookup the
        // server already did at follow time — bypass the per-card metadata
        // round-trip and pass it directly.
        tmdbVerified = true,
        title = follow.name,
        subtitle = subtitle,
        progress = null,
        progressColor = null,
        onClick = onClick,
        posterUrlOverride = tmdbPosterUrl(follow.posterPath, "w342"),
        topBadge = if (follow.newCount > 0) {
            {
                androidx.tv.material3.Surface(
                    shape = RoundedCornerShape(6.dp),
                    colors = androidx.tv.material3.SurfaceDefaults.colors(
                        containerColor = MaterialTheme.colorScheme.primary,
                    ),
                ) {
                    Text(
                        "${follow.newCount} nouveau${if (follow.newCount > 1) "x" else ""}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onPrimary,
                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                    )
                }
            }
        } else null,
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun FeaturedCard(
    container: AppContainer,
    result: SearchResult,
    onClick: () -> Unit,
) {
    val subtitle = listOfNotNull(
        result.year?.toString(),
        result.seeders?.let { "$it seeders" },
    ).joinToString(" · ")
    PosterCard(
        container = container,
        tmdbId = result.tmdbId,
        // Trust torr9's tmdb_id on featured items — they're hand-curated
        // server-side, the wrong-poster risk that justifies tmdb_verified
        // gating on library entries doesn't apply here.
        tmdbVerified = result.tmdbId != null,
        title = result.title,
        subtitle = subtitle.ifEmpty { result.providerId },
        progress = null,
        progressColor = null,
        onClick = onClick,
        topBadge = if (result.freeleech) {
            {
                androidx.tv.material3.Surface(
                    shape = RoundedCornerShape(4.dp),
                    colors = androidx.tv.material3.SurfaceDefaults.colors(
                        containerColor = androidx.compose.ui.graphics.Color(0xFF10B981).copy(alpha = 0.85f),
                    ),
                ) {
                    Text(
                        "FL",
                        style = MaterialTheme.typography.labelSmall,
                        color = androidx.compose.ui.graphics.Color.White,
                        fontWeight = FontWeight.Bold,
                        modifier = Modifier.padding(horizontal = 5.dp, vertical = 1.dp),
                    )
                }
            }
        } else null,
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun CollectionCard(
    container: AppContainer,
    collection: CollectionListItem,
    onClick: () -> Unit,
) {
    val subtitle = buildString {
        if (collection.kind == "tv" && collection.episodeCount > 0) {
            append("${collection.episodeCount} ep")
        } else {
            append(formatBytes(collection.totalSizeBytes))
        }
        if (collection.torrentCount > 1) {
            append(" · ${collection.torrentCount} torrents")
        }
    }
    PosterCard(
        container = container,
        tmdbId = collection.tmdbId,
        // The collection's tmdb_id was either captured at ingest from a
        // verified search hit or matched server-side — trust it for the
        // poster fetch the same way TV-side WatchlistCard does.
        tmdbVerified = collection.tmdbId != null,
        title = prettifyFilename(collection.displayTitle),
        subtitle = subtitle,
        progress = null,
        progressColor = null,
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
        tmdbVerified = torrent.tmdbVerified,
        title = prettifyFilename(torrent.name ?: torrent.infohash.take(12)),
        subtitle = formatBytes(torrent.totalSizeBytes),
        progress = null,
        progressColor = null,
        onClick = onClick,
    )
}

/**
 * In-progress torrent card. Shows a download-themed (blue) progress bar
 * across the bottom of the poster and live "X% · 12 MB/s" telemetry as
 * the subtitle. Polls upstream every 3s via [HomeScreen]'s LaunchedEffect.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun DownloadingCard(
    container: AppContainer,
    torrent: TorrentView,
    onClick: () -> Unit,
) {
    val pct = (torrent.progressPct / 100f).coerceIn(0f, 1f)
    val speed = torrent.downloadSpeedBps
    val subtitle = if (torrent.error != null) {
        "Error"
    } else if (torrent.state.equals("paused", ignoreCase = true)) {
        "Paused · ${torrent.progressPct.toInt()}%"
    } else if (speed > 0) {
        "${torrent.progressPct.toInt()}% · ${formatSpeed(speed)}"
    } else {
        "${torrent.progressPct.toInt()}% · waiting…"
    }
    PosterCard(
        container = container,
        tmdbId = torrent.tmdbId,
        tmdbVerified = torrent.tmdbVerified,
        title = prettifyFilename(torrent.name ?: torrent.infohash.take(12)),
        subtitle = subtitle,
        progress = pct,
        progressColor = androidx.compose.ui.graphics.Color(0xFF3B82F6),
        onClick = onClick,
    )
}

private fun formatBytes(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0f MB", mb)
    return "$b B"
}

private fun formatSpeed(bps: Long): String {
    val mbs = bps / 1_000_000.0
    if (mbs >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f MB/s", mbs)
    val kbs = bps / 1_000.0
    return String.format(java.util.Locale.ROOT, "%.0f KB/s", kbs)
}

/**
 * Strip the file extension and turn the dot/underscore-separated tokens of a
 * release name into something human: `Silicon.Valley.S01E01.1080p.mkv` →
 * `Silicon Valley S01E01 1080p`. We don't try to be too clever — TMDB
 * metadata replaces this once the poster lookup completes anyway.
 */
private fun prettifyFilename(raw: String): String {
    val noExt = raw.substringBeforeLast('.', raw)
    return noExt.replace('.', ' ').replace('_', ' ').trim()
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun PosterCard(
    container: AppContainer,
    tmdbId: Long?,
    /** Server-validated `(tmdb_id, runtime ≈ probed duration)`. Until this
     *  is true we never call TMDB — wrong posters / titles were the bigger
     *  UX hit than no posters. */
    tmdbVerified: Boolean,
    title: String,
    subtitle: String,
    /** 0..1 — when non-null, draws a thin progress bar across the bottom of the poster. */
    progress: Float?,
    /** When `null`, defaults to the Material primary (good for watch progress); pass a
     *  custom color (e.g. blue) to differentiate download from watch. */
    progressColor: androidx.compose.ui.graphics.Color?,
    onClick: () -> Unit,
    /** Pre-resolved poster URL — skips the TMDB metadata roundtrip. Used by
     *  the Watchlist shelf where /api/me/follows already returns the poster
     *  path inline. When `null`, falls back to the regular TMDB lookup. */
    posterUrlOverride: String? = null,
    /** Optional top-right overlay (e.g., "X nouveaux" badge). Renders on
     *  top of the poster. */
    topBadge: (@Composable () -> Unit)? = null,
) {
    var meta by remember(tmdbId, tmdbVerified) { mutableStateOf<TmdbMetadata?>(null) }
    LaunchedEffect(tmdbId, tmdbVerified, posterUrlOverride) {
        if (posterUrlOverride != null) return@LaunchedEffect
        if (!tmdbVerified || tmdbId == null) return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        meta = runCatching { container.apiFor(url).tmdbMetadata(tmdbId) }.getOrNull()
    }
    val posterUrl = posterUrlOverride ?: tmdbPosterUrl(meta?.posterPath, "w342")
    // Filename always wins for the title — see the rationale on
    // `tmdbVerified`. Even with a verified match the filename is what
    // the user dropped on disk and is most likely to recognise.
    val displayTitle = title
    val barColor = progressColor ?: MaterialTheme.colorScheme.primary

    Card(
        onClick = onClick,
        modifier = Modifier.width(140.dp),
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
                        contentDescription = displayTitle,
                        modifier = Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    // No-poster placeholder: the previous version showed a
                    // huge 2-letter monogram which looked broken. We now
                    // mimic a real poster — vertical gradient + a discrete
                    // film-strip icon, with the title typeset in the lower
                    // half so the card's identity is the *title*, not a
                    // letter.
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
                    Column(
                        Modifier.fillMaxSize().padding(horizontal = 12.dp, vertical = 16.dp),
                        verticalArrangement = Arrangement.SpaceBetween,
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        Text(
                            "🎬",
                            style = MaterialTheme.typography.headlineSmall,
                            color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.55f),
                        )
                        Text(
                            displayTitle,
                            style = MaterialTheme.typography.titleSmall,
                            color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.92f),
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 4,
                            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                        )
                    }
                }
                progress?.let { p ->
                    androidx.compose.foundation.layout.Box(
                        Modifier
                            .align(Alignment.BottomStart)
                            .fillMaxWidth()
                            .height(4.dp)
                            .background(androidx.compose.ui.graphics.Color.Black.copy(alpha = 0.5f)),
                    )
                    androidx.compose.foundation.layout.Box(
                        Modifier
                            .align(Alignment.BottomStart)
                            .fillMaxWidth(p)
                            .height(4.dp)
                            .background(barColor),
                    )
                }
                if (topBadge != null) {
                    Box(
                        Modifier
                            .align(Alignment.TopEnd)
                            .padding(6.dp),
                    ) {
                        topBadge()
                    }
                }
            }
            Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    displayTitle,
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
