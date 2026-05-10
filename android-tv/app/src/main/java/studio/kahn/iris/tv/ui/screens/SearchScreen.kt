package studio.kahn.iris.tv.ui.screens

import android.app.Activity
import android.content.Intent
import android.speech.RecognizerIntent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.tv.material3.Border
import androidx.tv.material3.Button
import androidx.tv.material3.ButtonDefaults
import androidx.tv.material3.Card
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AggregatedResults
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.SearchResult
import studio.kahn.iris.tv.data.TmdbSuggestion
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Spacing
import kotlin.math.sqrt

private const val PAGE_SIZE = 30

private enum class KindFilter(val label: String, val apiKind: String?) {
    All("All", null),
    Movies("Movies", "movie"),
    Series("Series", "tv"),
}

/**
 * Sort presets exposed in the chip row. `Recommended` is a client-side
 * composite (seeders ÷ size) that surfaces fast-to-process releases
 * first — the user's stated preference. The remaining presets pass
 * straight through to `/api/search?sort_by=…&order=…`.
 */
private enum class SortMode(val label: String) {
    Recommended("Recommended"),
    Seeders("Seeders"),
    Newest("Newest"),
    Smallest("Smallest"),
    Title("Title"),
}

private data class FetchKey(
    val q: String,
    val page: Int,
    val sort: SortMode,
    val kind: KindFilter,
)

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun SearchScreen(
    container: AppContainer,
    initialQuery: String? = null,
    autoPickTop: Boolean = false,
    onPickResult: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onPickTorrent: (infohash: String) -> Unit,
    onBack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val layout = LocalTvLayout.current

    var query by remember { mutableStateOf(initialQuery ?: "") }
    var submittedQuery by remember { mutableStateOf(initialQuery?.trim().orEmpty()) }
    var kind by remember { mutableStateOf(KindFilter.All) }
    var sort by remember { mutableStateOf(SortMode.Recommended) }
    var page by remember { mutableIntStateOf(1) }

    var data by remember { mutableStateOf<AggregatedResults?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var pending by remember { mutableStateOf(false) }
    var ingestingId by remember { mutableStateOf<String?>(null) }
    // Memoised TMDB lookup results keyed by extracted SCENE title. We
    // resolve the poster from the *release name* (after stripping year /
    // SxxExx / quality / language tokens) instead of trusting the
    // indexer's per-result `tmdb_id`, which torr9 frequently mistags
    // (Silicon Valley releases pointed at "The Burning Bed", etc.). One
    // entry per unique cleaned title — all S01/S02/etc. releases of
    // Silicon Valley share a single network call.
    val tmdbCache = remember { mutableStateMapOf<String, TmdbSuggestion?>() }

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
                submittedQuery = text.trim()
                page = 1
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

    // Fetch on every (q, page, sort, kind) change. Recommended sort
    // routes through the API as `seeders desc` — we then re-rank
    // client-side on the merged list.
    val key = FetchKey(submittedQuery, page, sort, kind)
    LaunchedEffect(key) {
        if (key.q.length < 2) {
            data = null
            return@LaunchedEffect
        }
        pending = true
        error = null
        try {
            val url = container.sessionStore.serverUrl.first()
            if (url == null) {
                error = "Not signed in"
                return@LaunchedEffect
            }
            val api: IrisApi = container.apiFor(url)
            val (sortBy, order) = when (sort) {
                SortMode.Recommended -> "seeders" to "desc"
                SortMode.Seeders -> "seeders" to "desc"
                SortMode.Newest -> "uploaded" to "desc"
                SortMode.Smallest -> "size" to "asc"
                SortMode.Title -> "title" to "asc"
            }
            val res = api.search(
                q = key.q,
                page = key.page,
                limit = PAGE_SIZE,
                sortBy = sortBy,
                order = order,
                kind = key.kind.apiKind,
            )
            val ranked = if (sort == SortMode.Recommended) {
                // Filter to video-shaped releases first (kind in
                // {movie, tv} or size ≥ 200 MB), then re-rank by
                // composite score. Books / music / samples drop out
                // entirely instead of dominating the top spots.
                res.copy(
                    results = res.results
                        .filter(::isLikelyVideo)
                        .sortedByDescending(::recommendedScore),
                )
            } else {
                res
            }
            data = ranked
            // Auto-pick the top hit on voice deep-link (MEDIA_PLAY_FROM_SEARCH).
            if (autoPickTop && page == 1 && ranked.results.isNotEmpty() && ingestingId == null) {
                val top = ranked.results.first()
                ingestingId = "${top.providerId}:${top.externalId}"
                ingestAndPlay(scope, container, top, onPickFile, onPickTorrent) { msg ->
                    error = msg
                    ingestingId = null
                }
            }
        } catch (e: Exception) {
            error = e.message ?: "Search failed"
        } finally {
            pending = false
        }
    }

    val totals = remember(data) {
        var count = 0L
        var pages = 0
        for (p in data?.providers.orEmpty()) {
            p.totalCount?.let { count += it }
            p.totalPages?.let { if (it > pages) pages = it }
        }
        count to pages
    }

    // Resolve TMDB posters from cleaned SCENE titles. Runs whenever the
    // result set changes, fires one TMDB multi-search per *unique* cleaned
    // title not already in the cache. Misses fall back to the indexer's
    // poster_url / placeholder gradient inside ResultCard.
    LaunchedEffect(data) {
        val results = data?.results.orEmpty()
        if (results.isEmpty()) return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        val api = container.apiFor(url)
        val unresolved = results
            .map { extractSceneTitle(it.title) }
            .filter { it.isNotBlank() && !tmdbCache.containsKey(it) }
            .distinct()
        for (name in unresolved) {
            val pick = runCatching { api.tmdbSearch(name) }.getOrNull().orEmpty()
            // Prefer hits matching the result's `kind` when the search
            // had a `kind` filter active; otherwise take the popularity
            // top.
            val target = pick.firstOrNull { it.kind == kind.apiKind }
                ?: pick.firstOrNull()
            tmdbCache[name] = target
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .padding(horizontal = layout.gutterHorizontal, vertical = layout.gutterVertical),
        verticalArrangement = Arrangement.spacedBy(Spacing.lg),
    ) {
        // --- Header ---
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "Search",
                style = MaterialTheme.typography.headlineLarge,
                color = MaterialTheme.colorScheme.primary,
                fontWeight = FontWeight.SemiBold,
            )
            Button(
                onClick = onBack,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
            ) { Text("← Back") }
        }

        // --- Input + Voice row ---
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Spacing.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Local helper so the input + the IME's "Search" key on
            // Android TV's leanback keyboard run the same handler. The
            // bare `imeAction = ImeAction.Search` flag wasn't enough —
            // it advertises the action to the IME but without a
            // `keyboardActions` callback the keypress was a no-op,
            // which is why the user had to dismiss the keyboard and
            // hit our app-side Search button.
            val submit = {
                if (!pending && query.trim().length >= 2) {
                    submittedQuery = query.trim()
                    page = 1
                }
            }
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                singleLine = true,
                placeholder = {
                    androidx.compose.material3.Text(
                        "Title, year, anything…",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                },
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { submit() }),
                // `textStyle` forces the typed-text colour, which
                // OutlinedTextField pulls from `LocalTextStyle` first,
                // not from `OutlinedTextFieldDefaults.colors`. Without
                // this the input rendered with the default text style's
                // colour (light-theme black on Android TV's stock IME)
                // and the user couldn't read what they were typing.
                textStyle = LocalTextStyle.current.copy(
                    color = MaterialTheme.colorScheme.onSurface,
                    fontSize = 18.sp,
                ),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedTextColor = MaterialTheme.colorScheme.onSurface,
                    unfocusedTextColor = MaterialTheme.colorScheme.onSurface,
                    focusedBorderColor = MaterialTheme.colorScheme.primary,
                    unfocusedBorderColor = MaterialTheme.colorScheme.surfaceVariant,
                    focusedContainerColor = MaterialTheme.colorScheme.surface,
                    unfocusedContainerColor = MaterialTheme.colorScheme.surface,
                    cursorColor = MaterialTheme.colorScheme.primary,
                ),
                modifier = Modifier.weight(1f).height(60.dp),
            )
            Button(
                onClick = { submit() },
                enabled = !pending && query.trim().length >= 2,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 24.dp, vertical = 16.dp),
            ) { Text(if (pending) "Searching…" else "Search") }
            Button(
                onClick = { launchVoice() },
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 16.dp),
            ) { Text("🎤  Voice") }
        }

        // --- Filter + Sort chips ---
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ChipGroup(
                label = "Type",
                options = KindFilter.values().toList(),
                value = kind,
                labelOf = { it.label },
                onChange = { kind = it; page = 1 },
            )
            Box(Modifier.width(Spacing.lg))
            ChipGroup(
                label = "Sort",
                options = SortMode.values().toList(),
                value = sort,
                labelOf = { it.label },
                onChange = { sort = it; page = 1 },
            )
        }

        // --- Results ---
        val rows = data?.results.orEmpty()
        when {
            submittedQuery.length < 2 -> EmptyHint(
                title = "Type a title, then press Search",
                body = "Or hit the 🎤 button — voice search is the fastest path on a remote.",
            )
            error != null -> ErrorBlock(message = error!!) {
                submittedQuery = query.trim().ifEmpty { submittedQuery }
                page = 1
            }
            pending && rows.isEmpty() -> SkeletonGrid(layout.gridPosterMin)
            !pending && rows.isEmpty() -> EmptyHint(
                title = "No results",
                body = "Try a different title or switch the kind / sort filters.",
            )
            else -> {
                ResultsHeader(
                    submittedQuery = submittedQuery,
                    totals = totals,
                    pending = pending,
                    providers = data?.providers.orEmpty().map { it.id },
                )
                Box(Modifier.weight(1f)) {
                    LazyVerticalGrid(
                        columns = GridCells.Adaptive(minSize = layout.gridPosterMin),
                        horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
                        verticalArrangement = Arrangement.spacedBy(Spacing.xl),
                        contentPadding = PaddingValues(vertical = Spacing.sm),
                    ) {
                        items(rows, key = { "${it.providerId}:${it.externalId}" }) { r ->
                            val cleaned = remember(r.title) { extractSceneTitle(r.title) }
                            ResultCard(
                                result = r,
                                resolvedPoster = tmdbCache[cleaned]?.posterPath,
                                onClick = {
                                    onPickResult(r.providerId, r.externalId, r.tmdbId, r.kind)
                                },
                            )
                        }
                    }
                }
                Pagination(
                    page = page,
                    totalPages = totals.second.coerceAtLeast(1),
                    pending = pending,
                    onPrev = { if (page > 1) page-- },
                    onNext = { page++ },
                )
            }
        }
    }
}

/** Below this, a release is almost certainly an ebook / music /
 *  sample rather than a video. Filters `Recommended` and floors the
 *  size term in the composite score (a 10 MB book with high seeders
 *  would otherwise out-score a 5 GB movie by 100×). */
private const val MIN_VIDEO_BYTES: Long = 200L * 1024 * 1024
private const val SIZE_FLOOR_GIB: Double = 0.5

/** Whitelist for `Recommended`: only releases the indexer classified
 *  as movie/tv (or, when kind is missing, anything bigger than the
 *  video size floor — catches uncategorised but plausibly-video hits
 *  without letting books through). */
private fun isLikelyVideo(r: SearchResult): Boolean {
    if (r.kind == "movie" || r.kind == "tv") return true
    return (r.sizeBytes ?: 0L) >= MIN_VIDEO_BYTES
}

/**
 * Composite score the Recommended sort uses. `seeders / sqrt(size_gib)`
 * favours small-file releases without crushing every 10 GiB encode out of
 * the top picks. The size term is floored at 0.5 GiB so a 10 MB hit
 * doesn't game the denominator. Falls back to raw seeders when size is
 * unknown so we don't sink usable hits to the bottom.
 */
private fun recommendedScore(r: SearchResult): Double {
    val seeders = (r.seeders ?: 0).toDouble()
    val sizeGiB = r.sizeBytes?.let { it.toDouble() / (1024.0 * 1024.0 * 1024.0) }
    return if (sizeGiB != null && sizeGiB > 0.0) {
        seeders / sqrt(maxOf(sizeGiB, SIZE_FLOOR_GIB))
    } else {
        seeders
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun <T> ChipGroup(
    label: String,
    options: List<T>,
    value: T,
    labelOf: (T) -> String,
    onChange: (T) -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
        Text(
            label.uppercase(),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        options.forEach { opt ->
            Chip(
                label = labelOf(opt),
                selected = opt == value,
                onClick = { onChange(opt) },
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Chip(label: String, selected: Boolean, onClick: () -> Unit) {
    val container = if (selected) MaterialTheme.colorScheme.primary
    else MaterialTheme.colorScheme.surfaceVariant
    val onContainer = if (selected) MaterialTheme.colorScheme.onPrimary
    else MaterialTheme.colorScheme.onSurface
    Surface(
        onClick = onClick,
        shape = ClickableSurfaceDefaults.shape(shape = RoundedCornerShape(999.dp)),
        colors = ClickableSurfaceDefaults.colors(
            containerColor = container,
            contentColor = onContainer,
            focusedContainerColor = MaterialTheme.colorScheme.primary,
            focusedContentColor = MaterialTheme.colorScheme.onPrimary,
        ),
        border = ClickableSurfaceDefaults.border(
            border = Border.None,
            focusedBorder = Border(
                border = androidx.compose.foundation.BorderStroke(
                    width = 2.dp,
                    color = MaterialTheme.colorScheme.primary,
                ),
                shape = RoundedCornerShape(999.dp),
            ),
        ),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 6.dp),
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ResultsHeader(
    submittedQuery: String,
    totals: Pair<Long, Int>,
    pending: Boolean,
    providers: List<String>,
) {
    val (count, _) = totals
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column {
            Text(
                "Results for \"$submittedQuery\"",
                style = MaterialTheme.typography.titleMedium,
            )
            val sub = buildString {
                if (count > 0) append("$count hits")
                if (providers.isNotEmpty()) {
                    if (isNotEmpty()) append(" · ")
                    append(providers.joinToString(", "))
                }
            }
            if (sub.isNotEmpty()) {
                Text(
                    sub,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (pending) {
            Text(
                "loading…",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ResultCard(
    result: SearchResult,
    /** TMDB poster path resolved upstream from the SCENE-cleaned release
     *  title — bypasses the indexer's per-result `tmdb_id` (frequently
     *  wrong, e.g. Silicon Valley → Burning Bed). When null we fall
     *  through to `result.posterUrl` (rarely populated) and finally to
     *  the placeholder gradient. */
    resolvedPoster: String?,
    onClick: () -> Unit,
) {
    val parsed = remember(result) { parseTags(result) }
    val poster: String? = tmdbPosterUrl(resolvedPoster, "w342") ?: result.posterUrl
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
            ) {
                if (poster != null) {
                    AsyncImage(
                        model = poster,
                        contentDescription = result.title,
                        modifier = Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    Box(
                        Modifier.fillMaxSize().background(
                            androidx.compose.ui.graphics.Brush.verticalGradient(
                                colors = listOf(
                                    MaterialTheme.colorScheme.primary.copy(alpha = 0.30f),
                                    Color(0xFF0B0D12),
                                ),
                            ),
                        ),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            if (result.kind == "tv") "📺" else "🎬",
                            style = MaterialTheme.typography.headlineMedium,
                            color = Color.White.copy(alpha = 0.55f),
                        )
                    }
                }
                // Top-right: provider badge + freeleech.
                Column(
                    Modifier
                        .align(Alignment.TopEnd)
                        .padding(6.dp),
                    horizontalAlignment = Alignment.End,
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    if (result.freeleech) {
                        BadgePill("FL", Color(0xFF10B981))
                    }
                    BadgePill(result.providerId, Color.Black.copy(alpha = 0.65f))
                }
                // Bottom strip: kind chip on the left.
                result.kind?.let { k ->
                    BadgePill(
                        label = if (k == "tv") "TV" else "Movie",
                        bg = Color.Black.copy(alpha = 0.65f),
                        modifier = Modifier
                            .align(Alignment.BottomStart)
                            .padding(6.dp),
                    )
                }
            }
            Column(
                Modifier.padding(10.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    result.title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                )
                // Year + quality tags.
                val sub = buildList {
                    result.year?.let { add(it.toString()) }
                    parsed.quality?.let { add(it) }
                }.joinToString(" · ")
                if (sub.isNotEmpty()) {
                    Text(
                        sub,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                // Stats line: seeders / leechers / size.
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        "↑ ${result.seeders ?: 0}",
                        style = MaterialTheme.typography.labelMedium,
                        color = Color(0xFF34D399),
                    )
                    Text(
                        "↓ ${result.leechers ?: 0}",
                        style = MaterialTheme.typography.labelMedium,
                        color = Color(0xFFFB7185),
                    )
                    result.sizeBytes?.let {
                        Text(
                            "·  ${formatSizeShort(it)}",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                // Language / sub chips. Tracker tags carry these (VFF,
                // VOSTFR, MULTI…); we surface the flat list so the user
                // can pick the right release without opening the detail.
                if (parsed.langs.isNotEmpty() || parsed.subs) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        parsed.langs.forEach { lang ->
                            BadgePill(lang, MaterialTheme.colorScheme.surfaceVariant, small = true)
                        }
                        if (parsed.subs) {
                            BadgePill("SUB", Color(0xFF6366F1), small = true)
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun BadgePill(
    label: String,
    bg: Color,
    small: Boolean = false,
    modifier: Modifier = Modifier,
) {
    val pad = if (small) 5.dp to 1.dp else 6.dp to 2.dp
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(if (small) 4.dp else 6.dp),
        colors = SurfaceDefaults.colors(containerColor = bg),
    ) {
        Text(
            label.uppercase(),
            style = if (small) MaterialTheme.typography.labelSmall
            else MaterialTheme.typography.labelSmall,
            color = Color.White,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.padding(horizontal = pad.first, vertical = pad.second),
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SkeletonGrid(minSize: androidx.compose.ui.unit.Dp) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = minSize),
        horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
        verticalArrangement = Arrangement.spacedBy(Spacing.xl),
        contentPadding = PaddingValues(vertical = Spacing.sm),
    ) {
        items(8) {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .aspectRatio(2f / 3f)
                        .background(
                            MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
                            RoundedCornerShape(12.dp),
                        ),
                )
                Box(
                    Modifier
                        .fillMaxWidth(0.7f)
                        .height(14.dp)
                        .background(
                            MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
                            RoundedCornerShape(4.dp),
                        ),
                )
                Box(
                    Modifier
                        .fillMaxWidth(0.45f)
                        .height(10.dp)
                        .background(
                            MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
                            RoundedCornerShape(4.dp),
                        ),
                )
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EmptyHint(title: String, body: String) {
    Column(
        Modifier
            .fillMaxSize()
            .padding(top = Spacing.xxl),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Spacing.sm),
    ) {
        Text(
            title,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurface,
        )
        Text(
            body,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ErrorBlock(message: String, onRetry: () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .padding(top = Spacing.xxl),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Spacing.md),
    ) {
        Text(
            message,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.error,
        )
        Button(
            onClick = onRetry,
            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
            contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
        ) { Text("Retry") }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Pagination(
    page: Int,
    totalPages: Int,
    pending: Boolean,
    onPrev: () -> Unit,
    onNext: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            "Page $page of $totalPages",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
            Button(
                onClick = onPrev,
                enabled = !pending && page > 1,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
            ) { Text("← Prev") }
            Button(
                onClick = onNext,
                enabled = !pending && page < totalPages,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
            ) { Text("Next →") }
        }
    }
}

// ===== SCENE name extraction ============================================

private val STOP_TOKEN = Regex(
    listOf(
        "^\\d{4}$",                                       // year
        "^[sS]\\d{1,2}([eE]\\d{1,3})?$",                  // S01 / S01E01
        "^[eE]\\d{1,3}$",                                  // E01
        "^season$",
        "^(480p|576p|720p|1080p|1440p|2160p|4k|uhd)$",
        "^(bluray|brrip|bdrip|webrip|web-?dl|web|hdtv|hdrip|dvdrip|hdlight|remux|hr-hdtv)$",
        "^(x264|x265|h\\.?264|h\\.?265|hevc|avc|av1|xvid|divx)$",
        "^(french|truefrench|vff|vfi|vfq|vf|vostfr|multi|english|eng|vo|vost)$",
        "^(complete|repack|proper|extended|directors?|uncut|hdr|hdr10|dv)$",
    ).joinToString("|"),
    RegexOption.IGNORE_CASE,
)

/**
 * Extract the canonical name from a SCENE-style release title. Walks
 * tokens (split on `.`, `_`, ` `, `[`, `(`) and stops at the first
 * "metadata" token (year, SxxExx, resolution, source, codec, language).
 * Whatever's before is the title we hand to TMDB.
 *
 * Mirrors the same logic in `web/src/pages/SearchPage.tsx`. Falls back
 * to the raw title when nothing parses (e.g. user-uploaded names without
 * standard separators), so we never end up with a blank lookup key.
 */
fun extractSceneTitle(raw: String): String {
    val tokens = raw
        .replace(Regex("[\\.\\_\\-\\[\\]\\(\\)]+"), " ")
        .split(Regex("\\s+"))
        .filter { it.isNotEmpty() }
    val head = mutableListOf<String>()
    for (t in tokens) {
        if (STOP_TOKEN.matches(t)) break
        head += t
    }
    val cleaned = head.joinToString(" ").trim()
    return if (cleaned.length >= 2) cleaned else raw.trim()
}

// ===== Tag parsing & helpers ============================================

private data class ParsedTags(
    val langs: List<String>,
    val subs: Boolean,
    val quality: String?,
)

private val LANGUAGE_SET = setOf(
    "VFF", "VFI", "VFQ", "VF", "VO", "VOSTFR", "MULTI", "TRUEFRENCH", "FRENCH",
    "ENG", "ENGLISH", "FR", "EN", "JP", "JAPANESE", "ES", "SPANISH",
)
private val SUB_HINTS = setOf("VOSTFR", "SUBS", "SUBBED", "SUBFRENCH")
private val QUALITY_HINTS = setOf(
    "2160P", "1080P", "720P", "480P", "4K", "UHD", "BLURAY", "BDRIP",
    "WEB-DL", "WEBDL", "WEB", "WEBRIP", "HDLIGHT", "DVDRIP", "REMUX",
    "HDR", "HDR10", "DV", "DOLBY", "HEVC", "X265", "X264", "AV1",
)

/**
 * Pull the language / subtitle / quality hints out of the indexer-supplied
 * tag list (and out of the title as a fallback — torr9 drops most of this
 * info into the title string for SCENE releases). Best-effort: missing
 * entries just mean we render fewer chips on the card.
 */
private fun parseTags(r: SearchResult): ParsedTags {
    val raw = (r.tags + listOf(r.title.uppercase()))
        .flatMap { it.uppercase().split(' ', '.', '-', '_', '[', ']', '(', ')', ',', '/') }
        .filter { it.isNotEmpty() }
        .toSet()
    val langs = raw.filter { it in LANGUAGE_SET }.distinct()
        .let { reorderLangs(it) }
        .take(3)
    val subs = raw.any { it in SUB_HINTS }
    val quality = raw.firstOrNull { it in QUALITY_HINTS }
        ?.let(::prettifyQuality)
    return ParsedTags(langs = langs, subs = subs, quality = quality)
}

private fun reorderLangs(langs: List<String>): List<String> {
    // "MULTI" first, then VFF/VF, then anything else; collapses near-
    // duplicates so we don't show "VF · FRENCH · TRUEFRENCH" together.
    val out = mutableListOf<String>()
    if ("MULTI" in langs) out += "MULTI"
    if (out.isEmpty()) {
        when {
            "VFF" in langs -> out += "VFF"
            "TRUEFRENCH" in langs -> out += "VFF"
            "FRENCH" in langs -> out += "VF"
            "VFI" in langs -> out += "VFI"
            "VFQ" in langs -> out += "VFQ"
            "VF" in langs -> out += "VF"
        }
    }
    if ("VOSTFR" in langs) out += "VOSTFR"
    val remaining = langs.filterNot { it in out || it in setOf("FRENCH", "TRUEFRENCH", "VF", "VFF", "VFI", "VFQ") }
    return (out + remaining).distinct()
}

private fun prettifyQuality(q: String): String = when (q) {
    "WEBDL", "WEB-DL", "WEB" -> "WEB-DL"
    "BDRIP" -> "BDRip"
    "X265", "HEVC" -> "HEVC"
    "X264" -> "x264"
    "BLURAY" -> "BluRay"
    "HDLIGHT" -> "HDLight"
    "DVDRIP" -> "DVDRip"
    "WEBRIP" -> "WEBRip"
    "HDR10" -> "HDR10"
    "DV" -> "Dolby Vision"
    else -> q
}

private fun formatSizeShort(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0f MB", mb)
    return "$b B"
}

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
            val api = container.apiFor(url)
            val res = api.ingest(
                studio.kahn.iris.tv.data.IngestRequest(
                    providerId = hit.providerId,
                    externalId = hit.externalId,
                    tmdbId = hit.tmdbId,
                )
            )
            val videoExts = listOf(
                ".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov",
                ".ts", ".mts", ".m2ts", ".wmv",
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
            onError(e.message ?: "Ingest failed")
        }
    }
}
