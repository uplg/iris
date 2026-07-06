package studio.kahn.iris.tv.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.widthIn
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.focusGroup
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusEvent
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxWidth
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
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
import androidx.compose.ui.text.style.TextOverflow
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
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.ContinueWatchingItem
import studio.kahn.iris.tv.data.CatalogCard
import studio.kahn.iris.tv.data.ForYou
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.LibraryResponse
import studio.kahn.iris.tv.data.PreferencesResponse
import studio.kahn.iris.tv.data.WatchlistItem
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.tmdbPosterUrl
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.LiveTv
import androidx.compose.material.icons.filled.Movie
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material.icons.filled.VideoLibrary
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.sp
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.MetaDot
import studio.kahn.iris.tv.ui.components.IrisWordmark
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.TvIconButton
import studio.kahn.iris.tv.ui.components.irisPosterBorder
import studio.kahn.iris.tv.ui.components.irisPosterGlow
import studio.kahn.iris.tv.ui.components.irisPosterPlaceholder
import studio.kahn.iris.tv.ui.components.irisPosterScale
import studio.kahn.iris.tv.ui.components.irisPosterShape
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing
import studio.kahn.iris.tv.ui.theme.irisAmbient

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
    /** Open the seedbox / raw-torrents management view. The Library
     *  shelf below stays as-is; this is the "it's a seedbox" surface. */
    onOpenTorrents: () -> Unit,
    /** Open the search screen. When `query` is non-null the search runs
     *  immediately with that string pre-filled. */
    onOpenSearch: (query: String?) -> Unit,
    /** Open the full Library grid (search + filters + sort). The Home
     *  shelf below is just a recent-N preview. */
    onOpenLibrary: () -> Unit,
    /** Route to the detail screen for a (provider, externalId) pair —
     *  same destination as picking a search result. Used by Featured
     *  cards so the user previews before deciding to follow / play.
     *  `kind` lets the detail screen render the Follow button only
     *  for TV results. */
    onPickResult: (providerId: String, externalId: String, tmdbId: Long?, kind: String?) -> Unit,
    /** Open the SeriesScreen for an existing follow. */
    onOpenSeries: (followId: String) -> Unit,
    /** Open the CollectionScreen for a Library collection. Lists all
     *  torrents + episodes belonging to that collection. */
    onOpenCollection: (collectionId: String) -> Unit,
    /** Open the organized "For You" page. */
    onOpenForYou: () -> Unit,
    /** Open the mood board ("Tonight"). */
    onOpenMoods: () -> Unit,
    /** Open the full watch-history list. */
    onOpenHistory: () -> Unit,
    /** Open the Live TV channel grid. */
    onOpenLiveTv: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var continueWatching by remember { mutableStateOf<List<ContinueWatchingItem>>(emptyList()) }
    // The Continue Watching tile whose manage sheet (remove / mark-watched)
    // is open, if any.
    var cwManageItem by remember { mutableStateOf<ContinueWatchingItem?>(null) }
    // Two separate states for the two shelves so a tick that only touches
    // a Downloading entry's progress/speed doesn't invalidate Library —
    // those cards stay frozen and skip recomposition entirely.
    var downloading by remember { mutableStateOf<List<TorrentView>>(emptyList()) }
    var library by remember { mutableStateOf<List<TorrentView>>(emptyList()) }
    var watchlist by remember { mutableStateOf<List<WatchlistItem>>(emptyList()) }
    var forYou by remember { mutableStateOf<ForYou?>(null) }
    var collections by remember { mutableStateOf<List<CollectionListItem>>(emptyList()) }
    // First-run onboarding: null until prefs load (or stays null on an
    // older server with no endpoint). `onboardingDismissed` lets the user
    // leave onboarding for this session without a refetch race.
    var preferences by remember { mutableStateOf<PreferencesResponse?>(null) }
    var onboardingDismissed by remember { mutableStateOf(false) }
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
                val watchlist: Result<List<WatchlistItem>>,
                val forYou: Result<ForYou>,
                val collections: Result<LibraryResponse>,
                val prefs: Result<PreferencesResponse>,
            )
            val fetch = withContext(Dispatchers.IO) {
                // Discovery / watchlist / library failures shouldn't
                // break the rest of the home — surface as empty
                // shelves, not as the global error banner.
                HomeFetch(
                    cw = runCatching { api.continueWatching() },
                    tor = runCatching { api.listTorrents() },
                    // Post-0.4: per-user Watchlist sourced from the
                    // user's series_follows rows (auto-created on
                    // grab). Replaces the legacy `listFollows()` which
                    // returned the same data through the C1 façade.
                    watchlist = runCatching { api.watchlist() },
                    forYou = runCatching { api.forYou() },
                    collections = runCatching { api.library("collections") },
                    // Onboarding gate. A 404 on an older server → failure
                    // → null → onboarding simply never shows.
                    prefs = runCatching { api.preferences() },
                )
            }
            val cw = fetch.cw
            val tor = fetch.tor
            val wl = fetch.watchlist
            val fy = fetch.forYou
            val coll = fetch.collections
            continueWatching = cw.getOrDefault(emptyList())
            val fresh = tor.getOrDefault(emptyList())
            val (newDl, newLib) = splitTorrents(fresh)
            downloading = newDl
            library = newLib
            watchlist = wl.getOrDefault(emptyList())
            forYou = fy.getOrNull()
            collections = (coll.getOrNull() as? LibraryResponse.CollectionsWrapper)?.value?.items.orEmpty()
            preferences = fetch.prefs.getOrNull()
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

    val layout = LocalTvLayout.current
    // First-run gate: a freshly-onboarded server returns prefs with
    // onboarding_completed=false → show the full-screen onboarding step in
    // place of Home until the user saves or skips.
    val needsOnboarding = preferences?.let { !it.onboardingCompleted } == true && !onboardingDismissed
    Box(Modifier.fillMaxSize().background(IrisColors.Background)) {
        // Ambient backlight wash (web `.ambient`) — a fixed, faint violet
        // glow behind the scrolling content. Decorative only.
        Box(Modifier.fillMaxSize().background(irisAmbient()))
        if (needsOnboarding) {
            OnboardingScreen(
                container = container,
                initialPrefs = preferences!!,
                onDone = { onboardingDismissed = true },
            )
        } else {
            HomeContent(
                layout = layout,
                error = error,
                loading = loading,
                continueWatching = continueWatching,
                downloading = downloading,
                library = library,
                watchlist = watchlist,
                forYou = forYou,
                collections = collections,
                container = container,
                onPickFile = onPickFile,
                onPickTorrent = onPickTorrent,
                onPickResult = onPickResult,
                onOpenSettings = onOpenSettings,
                onOpenTorrents = onOpenTorrents,
                onOpenSearch = onOpenSearch,
                onOpenLibrary = onOpenLibrary,
                onOpenCollection = onOpenCollection,
                onOpenForYou = onOpenForYou,
                onOpenMoods = onOpenMoods,
                onOpenHistory = onOpenHistory,
                onOpenLiveTv = onOpenLiveTv,
                onRetry = { loadVersion++ },
                onManageCw = { cwManageItem = it },
            )
        }

        // Held-select on a Continue Watching tile opens this manage sheet.
        cwManageItem?.let { item ->
            ContinueWatchingManageDialog(
                item = item,
                onDismiss = { cwManageItem = null },
                onRemove = {
                    cwManageItem = null
                    scope.launch {
                        val url = container.sessionStore.serverUrl.first()
                        if (url != null) {
                            runCatching {
                                // A TV series hides its whole collection; a
                                // movie / standalone drops its single row.
                                val body = if (item.collectionId != null) {
                                    studio.kahn.iris.tv.data.DismissCwRequest(
                                        collectionId = item.collectionId,
                                    )
                                } else {
                                    studio.kahn.iris.tv.data.DismissCwRequest(
                                        infohash = item.infohash,
                                        fileIdx = item.fileIdx,
                                    )
                                }
                                container.apiFor(url).dismissContinueWatching(body)
                            }
                            loadVersion++
                        }
                    }
                },
                onMarkWatched = {
                    cwManageItem = null
                    scope.launch {
                        val url = container.sessionStore.serverUrl.first()
                        if (url != null) {
                            runCatching {
                                container.apiFor(url).markWatched(item.infohash, item.fileIdx.toInt())
                            }
                            loadVersion++
                        }
                    }
                },
            )
        }
    }

    @Suppress("UNUSED_EXPRESSION") scope
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun HomeContent(
    layout: studio.kahn.iris.tv.ui.theme.TvLayout,
    error: String?,
    loading: Boolean,
    continueWatching: List<ContinueWatchingItem>,
    downloading: List<TorrentView>,
    library: List<TorrentView>,
    watchlist: List<WatchlistItem>,
    forYou: ForYou?,
    collections: List<CollectionListItem>,
    container: AppContainer,
    onPickFile: (String, Int) -> Unit,
    onPickTorrent: (String) -> Unit,
    onPickResult: (String, String, Long?, String?) -> Unit,
    onOpenSettings: () -> Unit,
    onOpenTorrents: () -> Unit,
    onOpenSearch: (String?) -> Unit,
    onOpenLibrary: () -> Unit,
    onOpenCollection: (String) -> Unit,
    onOpenForYou: () -> Unit,
    onOpenMoods: () -> Unit,
    onOpenHistory: () -> Unit,
    onOpenLiveTv: () -> Unit,
    onRetry: () -> Unit,
    onManageCw: (ContinueWatchingItem) -> Unit,
) {
    LazyColumn(
        modifier = Modifier
            .fillMaxSize(),
        // No TOP inset: the hero's backdrop bleeds to the screen's top edge
        // (the top bar rides on it). Bottom inset only. Horizontal gutter is
        // applied per-item (shelves manage their own so a focused card can
        // scale past the title column without clipping).
        contentPadding = PaddingValues(bottom = layout.gutterVertical),
        verticalArrangement = Arrangement.spacedBy(Spacing.xxl),
    ) {
        // When there's a resume pick, the brand + actions ride ON the hero
        // backdrop at the top, with the resume content pushed to the bottom of
        // the (tall) billboard — backdrop stuck to the screen top, no seam.
        // Otherwise the top bar is a standalone header.
        val resumePick = continueWatching.firstOrNull()
        val topBar: @Composable () -> Unit = {
            HomeTopBar(
                onOpenForYou = onOpenForYou,
                onOpenMoods = onOpenMoods,
                onOpenSearch = onOpenSearch,
                onOpenLibrary = onOpenLibrary,
                onOpenTorrents = onOpenTorrents,
                onOpenHistory = onOpenHistory,
                onOpenLiveTv = onOpenLiveTv,
                onOpenSettings = onOpenSettings,
            )
        }
        if (resumePick != null) {
            item(key = "hero") {
                ResumeHero(
                    container = container,
                    item = resumePick,
                    onResume = { onPickFile(resumePick.infohash, resumePick.fileIdx.toInt()) },
                    topBar = topBar,
                    modifier = Modifier.fillParentMaxHeight(0.78f),
                )
            }
        } else {
            item(key = "header") {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = layout.gutterHorizontal, vertical = layout.gutterVertical),
                ) { topBar() }
            }
        }

        if (error != null) {
            item(key = "error") {
                Row(
                    Modifier.padding(horizontal = layout.gutterHorizontal),
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(error, color = MaterialTheme.colorScheme.error)
                    Button(
                        onClick = onRetry,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    ) {
                        Text("Retry")
                    }
                }
            }
        } else if (loading && downloading.isEmpty() && library.isEmpty() && continueWatching.isEmpty()) {
            item(key = "loading") {
                Text(
                    "Loading library…",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = layout.gutterHorizontal),
                )
            }
        }

        if (continueWatching.isNotEmpty()) {
            item(key = "shelf-cw") {
                Shelf(title = "Continue Watching", eyebrow = "For you") {
                    items(continueWatching, key = { "${it.infohash}:${it.fileIdx}" }) { item ->
                        ContinueWatchingCard(
                            container = container,
                            item = item,
                            onClick = { onPickFile(item.infohash, item.fileIdx.toInt()) },
                            onLongClick = { onManageCw(item) },
                        )
                    }
                }
            }
        }

        // Library lives right after Continue Watching — it's the
        // user's actual content and the most-visited shelf, so it
        // needs to be in easy D-pad reach instead of buried below
        // discovery shelves.
        if (collections.isNotEmpty()) {
            item(key = "shelf-library") {
                // Recent-N preview only — the full, searchable/sortable grid
                // lives on the dedicated Library screen, opened from the
                // "See all" action on the shelf title (↑ from the row) or the
                // header icon. One horizontal row doesn't scale to a big lib.
                Shelf(
                    title = "My Library",
                    eyebrow = "On disk · ${collections.size}",
                    onSeeAll = onOpenLibrary,
                ) {
                    items(collections.take(12), key = { it.id }) { c ->
                        CollectionCard(
                            container = container,
                            collection = c,
                            // Open the dedicated Collection screen
                            // so the user gets the full episode /
                            // file list (mirrors web's
                            // `/collection/:id`). Previously this
                            // shot straight to the representative
                            // torrent, hiding the rest of the
                            // collection's content.
                            onClick = { onOpenCollection(c.id.toString()) },
                        )
                    }
                }
            }
        }

        if (watchlist.isNotEmpty()) {
            item(key = "shelf-watchlist") {
                Shelf(title = "My Watchlist", eyebrow = "Following · ${watchlist.size}") {
                    items(watchlist, key = { it.id }) { w ->
                        // Post-0.4: the Watchlist item's `id` IS
                        // the collection id (sourced from
                        // `/api/me/watchlist` which joins
                        // series_follows → collections). Route
                        // straight to the unified
                        // CollectionScreen — SeriesScreen is the
                        // retired surface kept only for legacy
                        // navigation flows.
                        WatchlistCard(
                            container = container,
                            item = w,
                            onClick = { onOpenCollection(w.id.toString()) },
                        )
                    }
                }
            }
        }

        forYou?.shelves?.forEach { shelf ->
            if (shelf.items.isNotEmpty()) {
                item(key = "shelf-${shelf.key}") {
                    Shelf(
                        title = shelf.title,
                        eyebrow = "Recommended",
                        onSeeAll = onOpenForYou,
                    ) {
                        items(shelf.items, key = { it.catalogId }) { card ->
                            CatalogCardTv(
                                container = container,
                                card = card,
                                // Same flow as the web: follow → collection,
                                // rolling-window card → detail/preview, lazy
                                // recommendation → title search.
                                onClick = {
                                    routeCatalogClick(card, onOpenCollection, onPickResult, onOpenSearch)
                                },
                            )
                        }
                    }
                }
            }
        }

        if (downloading.isNotEmpty()) {
            item(key = "shelf-downloading") {
                Shelf(title = "Downloading", eyebrow = "Active · ${downloading.size}") {
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

/**
 * Vertical shelf with title + horizontal row of cards.
 *
 * The `bringIntoViewRequester` + `onFocusEvent` plumbing is what
 * makes the parent `LazyColumn` actually scroll on D-pad. Compose's
 * default focus handling moves focus to off-canvas items but doesn't
 * move the viewport with it on TV — without this, you'd D-pad-down
 * past the third visible row, focus would land somewhere invisible,
 * and the screen would look frozen. Whenever any descendant gains
 * focus we ask the surrounding lazy column to scroll the whole
 * shelf into view.
 */
/**
 * Shelf hosts its own LazyRow so every shelf gets identical title
 * alignment + the same edge breathing room:
 *   * The title sits at `gutterHorizontal` from the screen edge.
 *   * The LazyRow itself extends full-width; `contentPadding`
 *     positions the first card at `gutterHorizontal` (matching the
 *     title) AND leaves room for the focus scale (~1.1×) to grow
 *     leftward without clipping at the screen edge.
 *   * Title → row gap bumped from 12.dp to 20.dp so the focused
 *     card's vertical scale doesn't crash into the title text.
 *
 * Callers pass a `LazyListScope` block — just `items(...) { … }` —
 * keeping the call sites short.
 */
@OptIn(ExperimentalTvMaterial3Api::class, ExperimentalComposeUiApi::class)
@Composable
internal fun Shelf(
    title: String,
    eyebrow: String? = null,
    /** When set, renders a focusable "See all →" action on the right of the
     *  shelf title (web `.shelf-head` link / the design's shelf-head arrows).
     *  Reached by pressing ↑ from the row — no walk to the end of the cards. */
    onSeeAll: (() -> Unit)? = null,
    content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit,
) {
    val layout = LocalTvLayout.current
    val requester = remember { BringIntoViewRequester() }
    val scope = rememberCoroutineScope()
    Column(
        modifier = Modifier
            .bringIntoViewRequester(requester)
            .onFocusEvent { state ->
                if (state.hasFocus) {
                    scope.launch { requester.bringIntoView() }
                }
            },
        verticalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        // Eyebrow (Inter, uppercase, tracked) + display title (Cal Sans), with
        // an optional compact "See all →" on the right (web `.shelf-head`
        // link). Only that button is focusable — pressing ↑ from any card is
        // redirected to it via `focusProperties` on the row, so it's reachable
        // without a card-walk and without a heavy full-width focus border.
        val seeAllFocus = remember { FocusRequester() }
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = layout.gutterHorizontal),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                if (eyebrow != null) Eyebrow(eyebrow)
                SectionTitle(title)
            }
            if (onSeeAll != null) {
                IrisButton(
                    "See all →",
                    onSeeAll,
                    variant = IrisButtonVariant.Ghost,
                    focusedScale = 1.04f,
                    modifier = Modifier.focusRequester(seeAllFocus),
                )
            }
        }
        LazyRow(
            // Redirect ↑ out of the row to the "See all" button. `up` only
            // catches the card spatially under the button (the last one); the
            // `exit` lambda fires for ANY child leaving the group upward, so
            // every card can reach it. Needs `focusGroup()` to take effect.
            modifier = if (onSeeAll != null) {
                Modifier
                    .focusGroup()
                    .focusProperties {
                        onExit = {
                            if (requestedFocusDirection == FocusDirection.Up) {
                                seeAllFocus.requestFocus()
                            }
                        }
                    }
            } else {
                Modifier
            },
            contentPadding = PaddingValues(horizontal = layout.gutterHorizontal),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
            content = content,
        )
    }
}

/**
 * Brand wordmark + persistent action icons. Rendered either as a standalone
 * home header or overlaid on the resume hero's backdrop (see [ResumeHero]).
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun HomeTopBar(
    onOpenForYou: () -> Unit,
    onOpenMoods: () -> Unit,
    onOpenSearch: (String?) -> Unit,
    onOpenLibrary: () -> Unit,
    onOpenTorrents: () -> Unit,
    onOpenHistory: () -> Unit,
    onOpenLiveTv: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        IrisWordmark(fontSize = 34.sp)
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            TvIconButton(
                icon = Icons.Filled.Search,
                contentDescription = "Search",
                onClick = { onOpenSearch(null) },
            )
            TvIconButton(
                icon = Icons.Filled.Movie,
                contentDescription = "Tonight",
                onClick = onOpenMoods,
            )
            TvIconButton(
                icon = Icons.Filled.Star,
                contentDescription = "For You",
                onClick = onOpenForYou,
            )
            TvIconButton(
                icon = Icons.Filled.VideoLibrary,
                contentDescription = "Library",
                onClick = onOpenLibrary,
            )
            TvIconButton(
                icon = Icons.Filled.Storage,
                contentDescription = "Seedbox / Torrents",
                onClick = onOpenTorrents,
            )
            TvIconButton(
                icon = Icons.Filled.LiveTv,
                contentDescription = "Live TV",
                onClick = onOpenLiveTv,
            )
            TvIconButton(
                icon = Icons.Filled.History,
                contentDescription = "Watch history",
                onClick = onOpenHistory,
            )
            TvIconButton(
                icon = Icons.Filled.Settings,
                contentDescription = "Settings",
                onClick = onOpenSettings,
            )
        }
    }
}

/**
 * Full-bleed resume billboard — the latest Continue-Watching pick.
 * 1:1 port of the web `ResumeHero` (`web/src/pages/HomePage.tsx`): backdrop
 * at 50% opacity under bottom + left scrims, eyebrow "Continue tonight ·
 * Resume", display title, dotted meta, overview, a Resume CTA, and a thin
 * progress bar with "Xh Ym left". TMDB art is only pulled once the server
 * has *verified* the match — a wrong backdrop on the giant hero is worse
 * than the bare release name.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ResumeHero(
    container: AppContainer,
    item: ContinueWatchingItem,
    onResume: () -> Unit,
    topBar: @Composable () -> Unit,
    modifier: Modifier = Modifier,
) {
    val layout = LocalTvLayout.current
    // Trust the server's tmdb_id (ignore the runtime-verified flag) — the same
    // pattern the Continue-Watching shelf cards and Detail use. Gating on
    // tmdb_verified left the hero backdrop blank almost every time, since the
    // flag is usually false even when the id is good (it's COALESCEd from the
    // parent collection's resolved id).
    var meta by remember(item.tmdbId) { mutableStateOf<MediaMetadata?>(null) }
    LaunchedEffect(item.tmdbId, item.kind) {
        if (item.tmdbId == null) return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        meta = runCatching { container.apiFor(url).tmdbMetadata(item.tmdbId, item.kind?.value) }.getOrNull()
    }
    val backdrop = tmdbBackdropUrl(meta?.backdropPath, "w1280")
    val title = meta?.title
        ?: prettifyFilename(item.filePath?.substringAfterLast('/') ?: item.torrentName)
    val progress = item.durationSeconds?.takeIf { it > 0 }
        ?.let { (item.positionSeconds / it).toFloat().coerceIn(0f, 1f) } ?: 0f
    val remaining = item.durationSeconds?.takeIf { it > 0 }
        ?.let { (it - item.positionSeconds).coerceAtLeast(0.0) } ?: 0.0
    val metaParts = listOfNotNull(
        meta?.year?.toString(),
        if (item.kind == MediaKind.tv) "Series" else "Movie",
        meta?.numberOfSeasons?.let { "$it seasons" },
    )

    // The backdrop fills the whole hero (stuck to the screen's top edge). The
    // top bar rides at the very top; a `Spacer(weight)` pushes the resume
    // content to the BOTTOM so it never collides with the branding/nav.
    Box(modifier.fillMaxWidth()) {
        if (backdrop != null) {
            AsyncImage(
                model = backdrop,
                contentDescription = title,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
                alpha = 0.5f,
            )
        } else {
            Box(Modifier.fillMaxSize().background(irisPosterPlaceholder()))
        }
        // Top fade (keeps the overlaid top bar legible), bottom fade (grounds
        // the content), left fade (web hero gradients).
        Box(
            Modifier.fillMaxSize().background(
                Brush.verticalGradient(
                    0.0f to IrisColors.Background.copy(alpha = 0.65f),
                    0.25f to Color.Transparent,
                    0.55f to Color.Transparent,
                    1.0f to IrisColors.Background,
                ),
            ),
        )
        Box(
            Modifier.fillMaxSize().background(
                Brush.horizontalGradient(0f to IrisColors.Background, 0.7f to Color.Transparent),
            ),
        )
        Column(
            Modifier
                .fillMaxSize()
                .padding(
                    start = layout.gutterHorizontal,
                    end = layout.gutterHorizontal,
                    top = layout.gutterVertical,
                    bottom = layout.gutterVertical,
                ),
        ) {
            topBar()
            Spacer(Modifier.weight(1f))
            Column(
                Modifier.fillMaxWidth(0.62f),
                verticalArrangement = Arrangement.spacedBy(Spacing.md),
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Eyebrow("Continue tonight", color = IrisColors.Brand)
                    Eyebrow("· Resume")
                }
                Text(
                    title,
                    style = MaterialTheme.typography.displaySmall,
                    color = IrisColors.Foreground,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                if (metaParts.isNotEmpty()) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(14.dp),
                    ) {
                        metaParts.forEachIndexed { i, m ->
                            if (i > 0) MetaDot()
                            Text(m, style = MaterialTheme.typography.bodyMedium, color = IrisColors.MutedForeground)
                        }
                    }
                }
                meta?.overview?.takeIf { it.isNotBlank() }?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodyMedium,
                        color = IrisColors.MutedForeground,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                IrisButton("Resume", onResume, icon = Icons.Filled.PlayArrow)
                if (progress > 0f) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(14.dp),
                    ) {
                        Box(
                            Modifier
                                .width(220.dp)
                                .height(4.dp)
                                .background(IrisColors.Elev2, RoundedCornerShape(Radius.pill)),
                        ) {
                            Box(
                                Modifier
                                    .fillMaxWidth(progress)
                                    .height(4.dp)
                                    .background(IrisColors.Brand, RoundedCornerShape(Radius.pill)),
                            )
                        }
                        if (remaining > 0) {
                            Text(
                                fmtLeft(remaining),
                                style = MaterialTheme.typography.bodySmall,
                                color = IrisColors.FgDim,
                            )
                        }
                    }
                }
            }
        }
    }
}

private fun fmtLeft(seconds: Double): String {
    val total = seconds.toLong()
    val h = total / 3600
    val m = (total % 3600) / 60
    val s = total % 60
    return if (h > 0) {
        "${h}h ${m.toString().padStart(2, '0')}m left"
    } else {
        "$m:${s.toString().padStart(2, '0')} left"
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ContinueWatchingCard(
    container: AppContainer,
    item: ContinueWatchingItem,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
) {
    val rawTitle = item.filePath?.substringAfterLast('/') ?: item.torrentName
    val pct = item.durationSeconds?.takeIf { it > 0 }
        ?.let { ((item.positionSeconds / it).toFloat()).coerceIn(0f, 1f) }
    PosterCard(
        onLongClick = onLongClick,
        container = container,
        tmdbId = item.tmdbId,
        // Trust the server's tmdb_id — `playback::continue_watching`
        // already COALESCEs from the parent collection's value
        // (which the SCENE backfill resolved). The runtime-verified
        // flag is irrelevant for poster display.
        tmdbVerified = item.tmdbId != null,
        kindHint = item.kind?.value,
        // Original release / file name verbatim. We don't strip
        // tokens — episode numbers (SxxExx) and quality markers stay
        // visible; the marquee scroll on PosterCard handles long
        // strings without truncating the SxxExx out of view.
        title = rawTitle,
        marqueeTitle = true,
        subtitle = when {
            item.nextUp -> "Up next"
            pct != null -> "${(pct * 100).toInt()}% watched"
            else -> "Just started"
        },
        progress = pct,
        progressColor = null, // primary = watch progress
        onClick = onClick,
    )
}

/** Manage sheet for a Continue Watching tile — focusable overlay with
 *  "Mark as watched" / "Remove from Continue Watching". tv-material has no
 *  built-in dialog, so it's a scrim + centered card; Back or the scrim
 *  dismisses, and the first button grabs focus on open. */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ContinueWatchingManageDialog(
    item: ContinueWatchingItem,
    onDismiss: () -> Unit,
    onRemove: () -> Unit,
    onMarkWatched: () -> Unit,
) {
    BackHandler(enabled = true, onBack = onDismiss)
    val title = item.filePath?.substringAfterLast('/') ?: item.torrentName
    val firstFocus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { firstFocus.requestFocus() } }
    Box(
        Modifier
            .fillMaxSize()
            .background(androidx.compose.ui.graphics.Color.Black.copy(alpha = 0.7f))
            .clickable(
                interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() },
                indication = null,
                onClick = onDismiss,
            ),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier
                .widthIn(max = 460.dp)
                .background(IrisColors.Elev2, RoundedCornerShape(Radius.lg))
                .padding(Spacing.xl),
            verticalArrangement = Arrangement.spacedBy(Spacing.md),
        ) {
            Eyebrow("Continue Watching")
            androidx.tv.material3.Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                color = IrisColors.Foreground,
                maxLines = 2,
                overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
            )
            IrisButton(
                if (item.nextUp) "Mark watched & skip" else "Mark as watched",
                onMarkWatched,
                modifier = Modifier.fillMaxWidth().focusRequester(firstFocus),
            )
            IrisButton(
                "Remove from Continue Watching",
                onRemove,
                variant = IrisButtonVariant.Ghost,
                modifier = Modifier.fillMaxWidth(),
            )
            IrisButton(
                "Cancel",
                onDismiss,
                variant = IrisButtonVariant.Ghost,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun WatchlistCard(
    container: AppContainer,
    item: WatchlistItem,
    onClick: () -> Unit,
) {
    // Post-0.4 Watchlist tile — server-provided poster (tmdb_verified
    // gating handled server-side), `new_count` shows episodes the
    // indexer surfaced since this user's last visit.
    val subtitle = if (item.newCount > 0) "${item.newCount} new" else "In your library"
    PosterCard(
        container = container,
        tmdbId = item.tmdbId,
        tmdbVerified = item.posterPath != null,
        title = item.name,
        subtitle = subtitle,
        progress = null,
        progressColor = null,
        onClick = onClick,
        posterUrlOverride = tmdbPosterUrl(item.posterPath, "w342"),
        topBadge = if (item.newCount > 0) {
            {
                androidx.tv.material3.Surface(
                    shape = RoundedCornerShape(6.dp),
                    colors = androidx.tv.material3.SurfaceDefaults.colors(
                        containerColor = MaterialTheme.colorScheme.primary,
                    ),
                ) {
                    Text(
                        "${item.newCount} new",
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
internal fun CatalogCardTv(
    container: AppContainer,
    card: CatalogCard,
    onClick: () -> Unit,
) {
    val newCount = 0 // backend CatalogCard exposes no new_count
    // Always say what it is — Movie / Series, prefixed with "Anime" for the
    // anime catalogue (which mixes movies and series). Solves "can't tell a
    // series from a film" on the blended shelves.
    val kindLabel = if (card.kind == MediaKind.tv) "Series" else "Movie"
    val typeLabel = if (card.isAnime) "Anime · $kindLabel" else kindLabel
    val subtitle = listOfNotNull(typeLabel, card.year?.toString()).joinToString(" · ")
    PosterCard(
        container = container,
        tmdbId = card.tmdbId,
        // Posters are pre-resolved server-side (TMDB CDN / AniList cover);
        // only fall back to a kind-safe TMDB lookup when the URL is missing.
        tmdbVerified = card.posterUrl == null && card.tmdbId != null,
        title = card.title,
        subtitle = subtitle,
        note = card.reason,
        progress = null,
        progressColor = null,
        onClick = onClick,
        kindHint = card.kind.value,
        posterUrlOverride = card.posterUrl,
        topBadge = when {
            newCount > 0 -> {
                {
                    androidx.tv.material3.Surface(
                        shape = RoundedCornerShape(4.dp),
                        colors = androidx.tv.material3.SurfaceDefaults.colors(
                            containerColor = IrisColors.Brand.copy(alpha = 0.9f),
                        ),
                    ) {
                        Text(
                            "$newCount new",
                            style = MaterialTheme.typography.labelSmall,
                            color = IrisColors.OnBrand,
                            fontWeight = FontWeight.Bold,
                            modifier = Modifier.padding(horizontal = 5.dp, vertical = 1.dp),
                        )
                    }
                }
            }
            // Discreet seeder count for rolling-window cards (1 seeder is fine
            // — we never warn, only block 0 at grab). Mirrors the web card.
            (card.seeders ?: 0) > 0 -> {
                {
                    androidx.tv.material3.Surface(
                        shape = RoundedCornerShape(4.dp),
                        colors = androidx.tv.material3.SurfaceDefaults.colors(
                            containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.85f),
                        ),
                    ) {
                        Text(
                            "${card.seeders}↑",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurface,
                            modifier = Modifier.padding(horizontal = 5.dp, vertical = 1.dp),
                        )
                    }
                }
            }
            else -> null
        },
    )
}

/**
 * Route a "For You" card click, identically on the home shelf and the
 * organized For-You page. A followed series with new episodes opens its
 * collection; a rolling-window card (with a recommended-best release) opens
 * the same detail/preview screen as a search hit so the user sees it before
 * downloading; a lazy recommendation (no resolved release) falls back to a
 * title search.
 */
internal fun routeCatalogClick(
    card: CatalogCard,
    onOpenCollection: (String) -> Unit,
    onPickResult: (String, String, Long?, String?) -> Unit,
    onOpenSearch: (String) -> Unit,
) {
    val collectionId: String? = null // backend CatalogCard exposes no collection_id
    val providerId = card.providerId
    val externalId = card.externalId
    when {
        collectionId != null -> onOpenCollection(collectionId)
        card.availability == "available" && providerId != null && externalId != null ->
            onPickResult(providerId, externalId, card.tmdbId, card.kind.value)
        else -> onOpenSearch(card.title)
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun CollectionCard(
    container: AppContainer,
    collection: CollectionListItem,
    onClick: () -> Unit,
) {
    val subtitle = buildString {
        if (collection.kind == MediaKind.tv && collection.episodeCount > 0) {
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
        tmdbVerified = collection.tmdbId != null,
        // Pass the collection's kind so the server's lookup hits the
        // right TMDB namespace. Without this, an id collision between
        // `/movie/X` and `/tv/X` flipped the poster to an unrelated
        // entry.
        kindHint = collection.kind.value,
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
        kindHint = torrent.kind?.value,
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
    val pct = (torrent.progressPct.toFloat() / 100f).coerceIn(0f, 1f)
    val speed = torrent.downloadSpeedBps
    val subtitle = if (torrent.error != null) {
        "Error"
    } else if (torrent.state == TorrentState.paused) {
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
        kindHint = torrent.kind?.value,
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

/** Hold duration (ms) that turns a DPAD-center press into a "long press". */
private const val LONG_PRESS_MS = 400L

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
    /** `"movie"` / `"tv"` — disambiguates TMDB's separate id namespaces.
     *  Without it, a `/movie/X` lookup wins arbitrarily over `/tv/X` on
     *  the server and we end up showing a stranger's poster. */
    kindHint: String? = null,
    /** Pre-resolved poster URL — skips the TMDB metadata roundtrip. Used by
     *  the Watchlist shelf where /api/me/follows already returns the poster
     *  path inline. When `null`, falls back to the regular TMDB lookup. */
    posterUrlOverride: String? = null,
    /** When true, the title scrolls horizontally on a single line if it
     *  overflows (Compose `basicMarquee`). Used by Continue Watching
     *  cards where we want the full episode filename visible without
     *  truncation. */
    marqueeTitle: Boolean = false,
    /** Optional top-right overlay (e.g., "X new" badge). Renders on
     *  top of the poster. */
    topBadge: (@Composable () -> Unit)? = null,
    /** Tiny brand-accent line under the subtitle — a recommendation "why"
     *  ("Matches your taste"). Mirrors the web card's `note`. Omitted when
     *  null/blank. */
    note: String? = null,
    /** Held-select ("long press") on the focused card — opens a manage menu
     *  (Continue Watching uses it for remove / mark-watched). Null = no menu. */
    onLongClick: (() -> Unit)? = null,
) {
    var meta by remember(tmdbId, tmdbVerified) { mutableStateOf<MediaMetadata?>(null) }
    LaunchedEffect(tmdbId, tmdbVerified, posterUrlOverride, kindHint) {
        if (posterUrlOverride != null) return@LaunchedEffect
        if (!tmdbVerified || tmdbId == null) return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        meta = runCatching {
            container.apiFor(url).tmdbMetadata(tmdbId, kindHint)
        }.getOrNull()
    }
    val posterUrl = posterUrlOverride ?: tmdbPosterUrl(meta?.posterPath, "w342")
    // Filename always wins for the title — see the rationale on
    // `tmdbVerified`. Even with a verified match the filename is what
    // the user dropped on disk and is most likely to recognise.
    val displayTitle = title
    val barColor = progressColor ?: MaterialTheme.colorScheme.primary

    val layout = LocalTvLayout.current
    val posterShape = RoundedCornerShape(Radius.poster)
    // Held-select ("long press") detection. We measure the DPAD_CENTER
    // down→up hold duration rather than relying on key-repeat events — most TV
    // remotes / the emulator DON'T repeat a held center, which is why the old
    // repeatCount approach never fired. `onPreviewKeyEvent` runs in the tunnel
    // phase, BEFORE the Card's own click handling, so consuming the key-up on a
    // long hold suppresses the click. We fire on RELEASE (not while held) so
    // the same press can't also activate the menu that just opened.
    var centerDownAt by remember { mutableStateOf(0L) }
    val longPressMod = if (onLongClick != null) {
        Modifier.onPreviewKeyEvent { ev ->
            val ne = ev.nativeKeyEvent
            val isSelect = ne.keyCode == android.view.KeyEvent.KEYCODE_DPAD_CENTER ||
                ne.keyCode == android.view.KeyEvent.KEYCODE_ENTER ||
                ne.keyCode == android.view.KeyEvent.KEYCODE_NUMPAD_ENTER
            if (!isSelect) return@onPreviewKeyEvent false
            when (ne.action) {
                android.view.KeyEvent.ACTION_DOWN -> {
                    if (ne.repeatCount == 0) centerDownAt = ne.eventTime
                    false
                }
                android.view.KeyEvent.ACTION_UP -> {
                    val held = if (centerDownAt > 0L) ne.eventTime - centerDownAt else 0L
                    centerDownAt = 0L
                    if (held >= LONG_PRESS_MS) {
                        onLongClick()
                        true // consume the up → the Card's onClick won't fire
                    } else {
                        false
                    }
                }
                else -> false
            }
        }
    } else {
        Modifier
    }
    Card(
        onClick = onClick,
        modifier = Modifier.width(layout.shelfPosterWidth).then(longPressMod),
        shape = irisPosterShape(posterShape),
        scale = irisPosterScale(),
        border = irisPosterBorder(posterShape),
        glow = irisPosterGlow(),
        colors = CardDefaults.colors(containerColor = IrisColors.Card),
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
                    // No-poster placeholder (web `.poster .fallback`): a
                    // brand-tinted diagonal wash with the title typeset in the
                    // Cal Sans display face along the lower edge, so the card's
                    // identity is the *title*, not a monogram.
                    Box(
                        Modifier
                            .fillMaxSize()
                            .background(irisPosterPlaceholder()),
                    )
                    Box(
                        Modifier.fillMaxSize().padding(14.dp),
                        contentAlignment = Alignment.BottomStart,
                    ) {
                        Text(
                            displayTitle,
                            style = MaterialTheme.typography.headlineSmall,
                            color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.92f),
                            maxLines = 3,
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
                if (marqueeTitle) {
                    // Single-line marquee scroll for filenames like
                    // `Show.S01E04.1080p.MULTI.x264-XYZ.mkv` where
                    // truncating to 2 lines hid the SxxExx part.
                    Text(
                        displayTitle,
                        style = MaterialTheme.typography.titleSmall,
                        maxLines = 1,
                        modifier = Modifier.basicMarquee(
                            iterations = Int.MAX_VALUE,
                            initialDelayMillis = 1500,
                            repeatDelayMillis = 1500,
                        ),
                    )
                } else {
                    Text(
                        displayTitle,
                        style = MaterialTheme.typography.titleSmall,
                        maxLines = 2,
                    )
                }
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall.copy(
                        fontFamily = studio.kahn.iris.tv.ui.theme.FontMono,
                    ),
                    color = IrisColors.FgDim,
                )
                if (!note.isNullOrBlank()) {
                    Text(
                        note,
                        style = MaterialTheme.typography.labelSmall,
                        color = IrisColors.Brand,
                        maxLines = 1,
                    )
                }
            }
        }
    }
}
