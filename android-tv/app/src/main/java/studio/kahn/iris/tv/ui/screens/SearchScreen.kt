package studio.kahn.iris.tv.ui.screens

import android.app.Activity
import android.content.Intent
import android.speech.RecognizerIntent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.basicMarquee
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
import androidx.compose.foundation.lazy.items as lazyListItems
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
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import android.content.Context
import android.view.inputmethod.InputMethodManager
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusManager
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.SoftwareKeyboardController
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
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
import studio.kahn.iris.tv.data.SearchViewMode
import studio.kahn.iris.tv.data.TmdbSuggestion
import studio.kahn.iris.tv.data.tmdbPosterUrl
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Search
import studio.kahn.iris.tv.ui.components.TvIconButton
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

/**
 * Saver for `rememberSaveable` of an enum: persists the variant by
 * `name` so the user's search query / filters survive navigating to a
 * result and pressing Back (the SEARCH back-stack entry keeps its
 * SavedStateRegistry alive — only a plain `remember` was losing it).
 */
private inline fun <reified T : Enum<T>> enumStateSaver(): Saver<T, String> =
    Saver(save = { it.name }, restore = { enumValueOf<T>(it) })

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

    // Persisted client-side (PrefsStore / its own DataStore) so the
    // user picks Grid vs List once, not every visit. collectAsState
    // drives the UI reactively; the toggle just writes the new value
    // and the flow re-emits.
    val viewMode by container.prefsStore.searchViewMode
        .collectAsState(initial = SearchViewMode.GRID)

    // rememberSaveable, not remember: navigating to a result detail
    // disposes this composable. With plain `remember` the query +
    // filters were lost, so pressing Back (UI or remote) dumped the
    // user on an empty search bar — they had to retype everything every
    // time. Saveable state is restored from the SEARCH back-stack
    // entry; the fetch LaunchedEffect below then re-runs and repopulates
    // the results. The search only clears when the user actually leaves
    // the screen or clears it themselves.
    var query by rememberSaveable { mutableStateOf(initialQuery ?: "") }
    var submittedQuery by rememberSaveable {
        mutableStateOf(initialQuery?.trim().orEmpty())
    }
    var kind by rememberSaveable(stateSaver = enumStateSaver<KindFilter>()) {
        mutableStateOf(KindFilter.All)
    }
    var sort by rememberSaveable(stateSaver = enumStateSaver<SortMode>()) {
        mutableStateOf(SortMode.Recommended)
    }
    var page by rememberSaveable { mutableIntStateOf(1) }

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
    // Keyed by `(cleaned title, result.kind)` rather than title alone:
    // a Movies-vs-Series mismatch on the same SCENE name (e.g. a film and
    // a series sharing a slug) used to share one cache entry, so the
    // wrong poster carried over to half the rows. Mirrors the web's
    // `["tmdb-by-title", cleaned, result.kind ?? "any"]` query key.
    val tmdbCache = remember { mutableStateMapOf<Pair<String, String?>, TmdbSuggestion?>() }

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
        // One resolve call per distinct (release title, kind). The
        // backend parses + scores by kind + year and caches 30d, so this
        // is both correct (no more "Pride" → "Pride and Prejudice"
        // popularity collisions) and cheap (shared server-side cache).
        val unresolved = results
            .map { it.title to it.kind }
            .filter { (title, _) -> title.isNotBlank() }
            .filter { it !in tmdbCache }
            .distinct()
        for ((title, kind) in unresolved) {
            tmdbCache[title to kind] =
                runCatching { api.tmdbResolve(title, kind) }.getOrNull()
        }
    }

    val focusManager: FocusManager = LocalFocusManager.current
    val keyboard: SoftwareKeyboardController? = LocalSoftwareKeyboardController.current
    val context = LocalContext.current
    Column(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .padding(horizontal = layout.gutterHorizontal, vertical = Spacing.md),
        verticalArrangement = Arrangement.spacedBy(Spacing.sm),
    ) {
        // No "Search" header and no Back button — the remote already
        // has a Back button and the screen's purpose is obvious from
        // the focused input. Every dp saved here is a poster row
        // the user can see above the fold.

        // --- Input + chips on a single dense row ---
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Spacing.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // The leanback (Android TV) on-screen keyboard is a
            // separate system Activity — Compose's
            // `SoftwareKeyboardController.hide()` doesn't always
            // dismiss it, so we layer three strategies:
            //   1. Move focus to the Search button (non-text target →
            //      the IME has no reason to stay attached).
            //   2. Compose's `keyboard?.hide()` (best-effort).
            //   3. The system `InputMethodManager.hideSoftInputFromWindow`
            //      against the current window token — the same call
            //      legacy Android Views use, and the only one that
            //      reliably dismisses the leanback keyboard activity
            //      when the IME action button is the one tapped.
            val searchBtnFocus = remember { FocusRequester() }
            val dismissImeViaSystem = {
                val activity = context as? android.app.Activity
                val token = activity?.window?.decorView?.windowToken
                if (token != null) {
                    val imm = activity.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
                    imm?.hideSoftInputFromWindow(token, 0)
                }
            }
            val submit = {
                if (!pending && query.trim().length >= 2) {
                    submittedQuery = query.trim()
                    page = 1
                }
                runCatching { searchBtnFocus.requestFocus() }
                keyboard?.hide()
                dismissImeViaSystem()
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
                // Input keeps its own column at a sensible width, NOT
                // stretching to fill — leaves room for the Type chips
                // on the same row. No fixed height: M3 OutlinedTextField
                // needs ~56 dp internally (label + content + bottom
                // padding) and a smaller forced height clipped the
                // typed text.
                modifier = Modifier.width(420.dp),
            )
            TvIconButton(
                icon = Icons.Filled.Search,
                contentDescription = if (pending) "Searching" else "Search",
                enabled = !pending && query.trim().length >= 2,
                onClick = { submit() },
                modifier = Modifier.focusRequester(searchBtnFocus),
            )
            TvIconButton(
                icon = Icons.Filled.Mic,
                contentDescription = "Voice search",
                onClick = { launchVoice() },
            )
            Box(Modifier.width(Spacing.lg))
            // Type chips share the input row to save a vertical line.
            // Sort gets its own thin row below.
            ChipGroup(
                label = "Type",
                options = KindFilter.values().toList(),
                value = kind,
                labelOf = { it.label },
                onChange = { kind = it; page = 1 },
            )
        }

        // --- Sort chips + view-mode toggle on a thin row ---
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Spacing.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ChipGroup(
                label = "Sort",
                options = SortMode.values().toList(),
                value = sort,
                labelOf = { it.label },
                onChange = { sort = it; page = 1 },
            )
            Box(Modifier.weight(1f))
            ChipGroup(
                label = "View",
                options = listOf(SearchViewMode.GRID, SearchViewMode.LIST),
                value = viewMode,
                labelOf = { if (it == SearchViewMode.GRID) "Grid" else "List" },
                onChange = { scope.launch { container.prefsStore.setSearchViewMode(it) } },
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
            pending && rows.isEmpty() -> SkeletonGrid(110.dp)
            !pending && rows.isEmpty() -> EmptyHint(
                title = "No results",
                body = "Try a different title or switch the kind / sort filters.",
            )
            else -> {
                // No results-header bar. Pagination at the bottom
                // already shows "Page X of Y" — that's enough; the
                // big "Results for X" / "N hits" line was eating a
                // poster row's worth of vertical space.
                // Compact poster minSize: smaller cards = more rows
                // visible at a glance.
                Box(Modifier.weight(1f)) {
                    when (viewMode) {
                        SearchViewMode.GRID -> LazyVerticalGrid(
                            columns = GridCells.Adaptive(minSize = 110.dp),
                            horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
                            verticalArrangement = Arrangement.spacedBy(Spacing.sm),
                            // Horizontal inset gives the scaled edge
                            // cards room to grow without clipping at
                            // the grid bounds.
                            contentPadding = PaddingValues(
                                horizontal = Spacing.xs,
                                vertical = Spacing.xs,
                            ),
                        ) {
                            items(
                                rows,
                                key = { "${it.providerId}:${it.externalId}" },
                            ) { r ->
                                ResultCard(
                                    result = r,
                                    resolvedPoster = tmdbCache[r.title to r.kind]?.posterPath,
                                    onClick = {
                                        onPickResult(r.providerId, r.externalId, r.tmdbId, r.kind)
                                    },
                                )
                            }
                        }
                        SearchViewMode.LIST -> LazyColumn(
                            verticalArrangement = Arrangement.spacedBy(Spacing.sm),
                            contentPadding = PaddingValues(vertical = Spacing.xs),
                        ) {
                            lazyListItems(
                                rows,
                                key = { "${it.providerId}:${it.externalId}" },
                            ) { r ->
                                ResultRow(
                                    result = r,
                                    resolvedPoster = tmdbCache[r.title to r.kind]?.posterPath,
                                    onClick = {
                                        onPickResult(r.providerId, r.externalId, r.tmdbId, r.kind)
                                    },
                                )
                            }
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
    var focused by remember { mutableStateOf(false) }
    Card(
        onClick = onClick,
        modifier = Modifier
            .fillMaxWidth()
            .onFocusChanged { focused = it.isFocused || it.hasFocus },
        shape = CardDefaults.shape(shape = RoundedCornerShape(12.dp)),
        // tv-material3's default focusedScale is 1.1 — on a tight
        // poster grid the edge cards blow past the gutter. A subtle
        // 1.05 keeps the focus cue without overflowing.
        scale = CardDefaults.scale(focusedScale = 1.05f),
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
                // Titles are routinely longer than the card. Off-focus:
                // 2 lines + ellipsis (keeps grid density). On-focus:
                // single-line marquee so the user can read the full
                // title of the card they're on — only one scrolls at a
                // time, never the whole grid.
                Text(
                    result.title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = if (focused) 1 else 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = if (focused) Modifier.basicMarquee() else Modifier,
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

/**
 * List-mode row. Shows exactly the same information as [ResultCard]
 * (poster, title, year · quality, seeders / leechers / size, kind,
 * language / sub + provider / freeleech badges) but laid out
 * horizontally so the full release title is readable at a glance —
 * the whole point of List mode. Compact w185 thumbnail keeps it light
 * for fast scrolling. Every `Text` sets an explicit colour: tv-material3
 * `Text` with none falls back to a black `LocalContentColor` here.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ResultRow(
    result: SearchResult,
    resolvedPoster: String?,
    onClick: () -> Unit,
) {
    val parsed = remember(result) { parseTags(result) }
    val poster: String? = tmdbPosterUrl(resolvedPoster, "w185") ?: result.posterUrl
    var focused by remember { mutableStateOf(false) }
    Card(
        onClick = onClick,
        modifier = Modifier
            .fillMaxWidth()
            .onFocusChanged { focused = it.isFocused || it.hasFocus },
        shape = CardDefaults.shape(shape = RoundedCornerShape(10.dp)),
        // Full-width row: any focusedScale > 1 overflows horizontally
        // by definition. Disable the zoom; focus is shown via the
        // card's focused container colour instead.
        scale = CardDefaults.scale(focusedScale = 1f),
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(8.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier
                    .width(46.dp)
                    .aspectRatio(2f / 3f)
                    .clip(RoundedCornerShape(6.dp)),
                contentAlignment = Alignment.Center,
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
                            style = MaterialTheme.typography.titleMedium,
                            color = Color.White.copy(alpha = 0.55f),
                        )
                    }
                }
            }
            Column(
                Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                // Full title: 2 lines at row width covers virtually
                // every release name; on focus it marquees so even the
                // longest scrolls fully into view.
                Text(
                    result.title,
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = if (focused) 1 else 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = if (focused) Modifier.basicMarquee() else Modifier,
                )
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
            }
            // Trailing badges — same set as the grid card.
            Row(
                horizontalArrangement = Arrangement.spacedBy(4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                result.kind?.let { k ->
                    BadgePill(
                        if (k == "tv") "TV" else "Movie",
                        Color.Black.copy(alpha = 0.65f),
                        small = true,
                    )
                }
                parsed.langs.forEach { lang ->
                    BadgePill(lang, MaterialTheme.colorScheme.surfaceVariant, small = true)
                }
                if (parsed.subs) {
                    BadgePill("SUB", Color(0xFF6366F1), small = true)
                }
                if (result.freeleech) {
                    BadgePill("FL", Color(0xFF10B981), small = true)
                }
                BadgePill(result.providerId, Color.Black.copy(alpha = 0.65f), small = true)
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
