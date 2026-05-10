package studio.kahn.iris.tv.ui.screens

import android.app.Activity
import android.content.Intent
import android.speech.RecognizerIntent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
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
import studio.kahn.iris.tv.data.IngestRequest
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.tmdbPosterUrl

/**
 * Text + voice search. Voice goes through the system [RecognizerIntent], so
 * Google Assistant on Android TV (long-press the mic button on the remote)
 * fills the field for us. Also handles the deep-link path: if the activity
 * was started by `MEDIA_PLAY_FROM_SEARCH`, [initialQuery] is non-null and we
 * auto-run the search + auto-pick the top result.
 *
 * Picking a result calls `POST /api/torrents` (ingest) with the search hit's
 * `tmdb_id` (so the torrent record carries the metadata for poster lookups
 * later) and then jumps to WatchScreen on the largest video file.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun SearchScreen(
    container: AppContainer,
    initialQuery: String? = null,
    autoPickTop: Boolean = false,
    /** Open the rich detail screen for a search hit. The detail screen
     *  owns the ingest + navigate-to-watch flow; SearchScreen no longer
     *  ingests directly, so the user always gets to see what they're
     *  about to grab (audio / sub / file size) before committing. */
    onPickResult: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onPickTorrent: (infohash: String) -> Unit,
    onBack: () -> Unit,
) {
    var query by remember { mutableStateOf(initialQuery ?: "") }
    var results by remember { mutableStateOf<List<SearchResult>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    var pending by remember { mutableStateOf(false) }
    var ingestingId by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    val voiceLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == Activity.RESULT_OK) {
            val text = result.data
                ?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)
                ?.firstOrNull()
                ?.takeIf { it.isNotBlank() }
            if (text != null) {
                query = text
                pending = true
                error = null
                scope.launch {
                    runSearch(container, text) { res, err ->
                        results = res
                        error = err
                        pending = false
                    }
                }
            }
        }
    }

    fun launchVoice() {
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(
                RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
            )
            putExtra(RecognizerIntent.EXTRA_PROMPT, "Search Iris")
        }
        runCatching { voiceLauncher.launch(intent) }.onFailure {
            error = "Voice search not available on this device"
        }
    }

    fun runQuery() {
        if (query.isBlank() || pending) return
        pending = true
        error = null
        scope.launch {
            runSearch(container, query) { res, err ->
                results = res
                error = err
                pending = false
            }
        }
    }

    // Deep-link path: auto-search + auto-play the top hit when the
    // activity was launched with MEDIA_PLAY_FROM_SEARCH.
    LaunchedEffect(initialQuery) {
        val q = initialQuery?.takeIf { it.isNotBlank() } ?: return@LaunchedEffect
        pending = true
        runSearch(container, q) { res, err ->
            results = res
            error = err
            pending = false
            if (autoPickTop && err == null && res.isNotEmpty()) {
                val top = res.first()
                ingestAndPlay(scope, container, top, onPickFile, onPickTorrent) { msg ->
                    error = msg
                    ingestingId = null
                }
                ingestingId = "${top.providerId}:${top.externalId}"
            }
        }
    }

    Column(
        Modifier.fillMaxSize().padding(40.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "Search".uppercase(),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                label = { androidx.compose.material3.Text("Title, year, etc.") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                modifier = Modifier.weight(1f),
            )
            Button(
                onClick = { runQuery() },
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 14.dp),
                enabled = !pending && query.isNotBlank(),
            ) {
                Text(if (pending) "Searching…" else "Search")
            }
            Button(
                onClick = { launchVoice() },
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 14.dp),
            ) {
                Text("🎤  Voice")
            }
        }

        error?.let {
            Text(it, color = MaterialTheme.colorScheme.error)
        }

        Box(Modifier.weight(1f)) {
            when {
                pending && results.isEmpty() -> Text("Searching…", color = MaterialTheme.colorScheme.onSurfaceVariant)
                results.isEmpty() && !pending && query.isNotBlank() -> Text(
                    "No results.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                else -> LazyVerticalGrid(
                    columns = GridCells.Fixed(5),
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                    verticalArrangement = Arrangement.spacedBy(20.dp),
                ) {
                    items(results, key = { "${it.providerId}:${it.externalId}" }) { r ->
                        ResultCard(
                            container = container,
                            result = r,
                            // Card click → push the rich detail screen
                            // where the user can read the synopsis,
                            // check audio/sub langs, then commit. The
                            // detail screen owns the ingest + navigate.
                            onClick = {
                                onPickResult(r.providerId, r.externalId, r.tmdbId, r.kind)
                            },
                        )
                    }
                }
            }
        }

        Button(
            onClick = onBack,
            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
            contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
        ) { Text("Back") }
    }
}

private suspend fun runSearch(
    container: AppContainer,
    q: String,
    onDone: (List<SearchResult>, String?) -> Unit,
) {
    try {
        val url = container.sessionStore.serverUrl.first()
            ?: return onDone(emptyList(), "Not signed in")
        val api: IrisApi = container.apiFor(url)
        val res = api.search(q = q, limit = 30)
        onDone(res.results, null)
    } catch (e: Exception) {
        onDone(emptyList(), e.message ?: "Search failed")
    }
}

/**
 * Ingest the search hit, then jump to WatchScreen on the largest video file
 * in the resulting torrent. The TMDB id is forwarded so the torrent record
 * carries the metadata for poster lookups later.
 */
private fun ingestAndPlay(
    scope: kotlinx.coroutines.CoroutineScope,
    container: AppContainer,
    hit: SearchResult,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onPickTorrent: (infohash: String) -> Unit,
    onError: (String) -> Unit,
) {
    scope.launch {
        try {
            val url = container.sessionStore.serverUrl.first()
                ?: return@launch onError("Not signed in")
            val api: IrisApi = container.apiFor(url)
            val res = api.ingest(
                IngestRequest(
                    providerId = hit.providerId,
                    externalId = hit.externalId,
                    tmdbId = hit.tmdbId,
                )
            )
            // Single-file → straight to play; multi-file (TV box set, anime
            // season) → DetailScreen so the user picks an episode.
            val videoExts = listOf(".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv")
            val videos = res.snapshot.files
                .filter { f -> videoExts.any { f.path.endsWith(it, ignoreCase = true) } }
            if (videos.size <= 1) {
                val idx = videos.maxByOrNull { f -> f.sizeBytes }?.index ?: 0
                onPickFile(res.snapshot.infohash, idx)
            } else {
                onPickTorrent(res.snapshot.infohash)
            }
        } catch (e: Exception) {
            onError(e.message ?: "Ingest failed")
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ResultCard(
    container: AppContainer,
    result: SearchResult,
    onClick: () -> Unit,
) {
    // TMDB poster lookup, mirrors PosterCard's behaviour. Search hits
    // come from the indexer pre-tagged with tmdb_id (when available);
    // we trust it here even though it's not "verified" — wrong-poster
    // risk is OK on a one-off card the user is about to inspect anyway.
    var meta by remember(result.tmdbId) { mutableStateOf<studio.kahn.iris.tv.data.TmdbMetadata?>(null) }
    LaunchedEffect(result.tmdbId) {
        if (result.tmdbId == null) return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        meta = runCatching { container.apiFor(url).tmdbMetadata(result.tmdbId) }.getOrNull()
    }
    val posterUrl = tmdbPosterUrl(meta?.posterPath, "w342")
    Card(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth(),
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
                        contentDescription = result.title,
                        modifier = Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
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
                    Text(
                        "🎬",
                        style = MaterialTheme.typography.headlineSmall,
                        color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.55f),
                    )
                }
                if (result.freeleech) {
                    androidx.tv.material3.Surface(
                        shape = RoundedCornerShape(4.dp),
                        colors = androidx.tv.material3.SurfaceDefaults.colors(
                            containerColor = androidx.compose.ui.graphics.Color(0xFF10B981).copy(alpha = 0.85f),
                        ),
                        modifier = Modifier
                            .align(Alignment.TopEnd)
                            .padding(6.dp),
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
            }
            Column(Modifier.padding(8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    result.title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                )
                Text(
                    listOfNotNull(
                        result.year?.toString(),
                        "↑ ${result.seeders ?: 0}",
                        result.sizeBytes?.let { formatBytesShort(it) },
                    ).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

private fun formatBytesShort(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1fG", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0fM", mb)
    return "${b}B"
}
