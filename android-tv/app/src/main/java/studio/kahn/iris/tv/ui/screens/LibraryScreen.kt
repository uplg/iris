package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.tv.material3.Border
import androidx.tv.material3.Card
import androidx.tv.material3.CardDefaults
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.first
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.CollectionListItem
import studio.kahn.iris.tv.data.LibraryResponse
import studio.kahn.iris.tv.data.MediaMetadata
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.SectionTitle
import studio.kahn.iris.tv.ui.components.irisPosterPlaceholder
import studio.kahn.iris.tv.ui.theme.FontMono
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing

private enum class LibKind(val label: String, val kind: String?) {
    All("All", null),
    Movies("Movies", "movie"),
    Series("Series", "tv"),
}

/**
 * Sort presets (web `/library`). "Recent" keeps the server's native list
 * order (it returns collections newest-first); A-Z and Size re-sort
 * client-side.
 */
private enum class LibrarySort(val label: String) {
    Recent("Recent"),
    Alpha("A-Z"),
    Size("Size"),
}

/**
 * Full library — the dedicated 2D grid the Home "My Library" shelf links
 * into. A single horizontal shelf is painful to D-pad through once the
 * household's library grows; this mirrors the web `/library` page: a
 * vertical poster grid with a search box, kind chips and sort presets so a
 * large collection collapses to what the user is actually after.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun LibraryScreen(
    container: AppContainer,
    onOpenCollection: (collectionId: String) -> Unit,
    onBack: () -> Unit,
) {
    val layout = LocalTvLayout.current
    var all by remember { mutableStateOf<List<CollectionListItem>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }

    var search by rememberSaveable { mutableStateOf("") }
    var kind by rememberSaveable { mutableStateOf(LibKind.All) }
    var sort by rememberSaveable { mutableStateOf(LibrarySort.Recent) }

    LaunchedEffect(Unit) {
        loading = true
        error = null
        try {
            val url = container.sessionStore.serverUrl.first()
                ?: run { error = "Not signed in"; loading = false; return@LaunchedEffect }
            val res = container.apiFor(url).library("collections")
            all = (res as? LibraryResponse.CollectionsWrapper)?.value?.items.orEmpty()
        } catch (e: Exception) {
            error = e.message ?: "Failed to load library"
        } finally {
            loading = false
        }
    }

    val visible = remember(all, search, kind, sort) {
        val q = search.trim().lowercase()
        val filtered = all.asSequence()
            .filter { kind.kind == null || it.kind.value == kind.kind }
            .filter { q.isEmpty() || it.displayTitle.lowercase().contains(q) }
            .toList()
        when (sort) {
            LibrarySort.Recent -> filtered // server order (newest-first)
            LibrarySort.Alpha -> filtered.sortedBy { it.displayTitle.lowercase() }
            LibrarySort.Size -> filtered.sortedByDescending { it.totalSizeBytes }
        }
    }

    // Land initial focus on a poster, NOT the search field — otherwise the
    // leanback keyboard pops open on entry. On return-from-detail we restore
    // focus to the card the user opened; on a fresh entry it's the first card.
    val gridState = rememberLazyGridState()
    val restoreFocus = remember { FocusRequester() }
    var didInitialFocus by remember { mutableStateOf(false) }
    // The collection the user opened. `rememberSaveable` survives navigation
    // (the NavBackStackEntry's SaveableStateHolder), so pressing Back from a
    // detail lands focus on THAT card instead of snapping to the first one.
    var lastOpenedId by rememberSaveable { mutableStateOf<String?>(null) }
    // Card to focus: the one we came from when it's still present under the
    // current filter, else the first card.
    val targetIndex = remember(visible, lastOpenedId) {
        lastOpenedId?.let { id -> visible.indexOfFirst { it.id.toString() == id } }?.takeIf { it >= 0 } ?: 0
    }
    LaunchedEffect(visible.isNotEmpty()) {
        if (didInitialFocus || visible.isEmpty()) return@LaunchedEffect
        // Bring the target into view first — a card far down the grid isn't
        // composed (so isn't focusable) until scrolled to. Then wait for it to
        // be laid out and request focus (a too-early request no-ops and the
        // text field wins the default focus).
        if (targetIndex > 0) runCatching { gridState.scrollToItem(targetIndex) }
        snapshotFlow { gridState.layoutInfo.visibleItemsInfo.any { it.index == targetIndex } }
            .first { it }
        runCatching { restoreFocus.requestFocus() }
        didInitialFocus = true
    }

    // The search field must NOT be focusable during the window where we're
    // about to land initial focus on the first card. Otherwise it's the first
    // focusable in the layout, grabs the default focus during the load+layout
    // gap, and pops the leanback IME before the card-focus request lands. This
    // hit on return-from-detail too: Back re-mounts the screen and re-fetches,
    // re-creating the gap (and `didInitialFocus` resets, so we re-lock). It
    // STAYS focusable when there's genuinely no card to focus — still loading
    // is locked, but an empty library / zero search matches keeps it usable so
    // the user can edit the query. Device-independent: removes the timing race
    // (a fast TV just never noticed the IME flash).
    val searchFocusable = !loading && (visible.isEmpty() || didInitialFocus)

    Box(Modifier.fillMaxSize().background(IrisColors.Background)) {
        Column(
            Modifier.fillMaxSize().padding(
                horizontal = layout.gutterHorizontal,
                vertical = layout.gutterVertical,
            ),
            verticalArrangement = Arrangement.spacedBy(Spacing.lg),
        ) {
            // Header — title + Back only (no eyebrow; the count lives on the
            // filter line).
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.Bottom,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                SectionTitle("Library")
                IrisButton("← Back", onBack, variant = IrisButtonVariant.Ghost)
            }

            // One compact filter line: search + Type chips + Sort chips + count.
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
            ) {
                OutlinedTextField(
                    value = search,
                    onValueChange = { search = it },
                    singleLine = true,
                    placeholder = {
                        androidx.compose.material3.Text("Search…", color = IrisColors.FgDim)
                    },
                    // Same TV-IME colour pinning as SearchScreen — without an
                    // explicit textStyle colour the typed text renders in the
                    // light-theme black on the leanback keyboard.
                    textStyle = LocalTextStyle.current.copy(
                        color = IrisColors.Foreground,
                        fontSize = 16.sp,
                    ),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedTextColor = IrisColors.Foreground,
                        unfocusedTextColor = IrisColors.Foreground,
                        focusedBorderColor = IrisColors.Brand,
                        unfocusedBorderColor = IrisColors.Elev2,
                        focusedContainerColor = IrisColors.Card,
                        unfocusedContainerColor = IrisColors.Card,
                        cursorColor = IrisColors.Brand,
                    ),
                    modifier = Modifier
                        .width(260.dp)
                        .focusProperties { canFocus = searchFocusable },
                )
                LibKind.entries.forEach { k ->
                    FilterChip(label = k.label, selected = kind == k) { kind = k }
                }
                Box(Modifier.width(Spacing.sm))
                LibrarySort.entries.forEach { s ->
                    FilterChip(label = s.label, selected = sort == s) { sort = s }
                }
                Box(Modifier.weight(1f))
                Text(
                    "${visible.size}/${all.size}",
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontMono),
                    color = IrisColors.FgDim,
                )
            }

            // Grid / status.
            when {
                loading && all.isEmpty() -> Text(
                    "Loading library…",
                    style = MaterialTheme.typography.bodyMedium,
                    color = IrisColors.MutedForeground,
                )
                error != null -> Text(error!!, color = MaterialTheme.colorScheme.error)
                visible.isEmpty() -> Text(
                    if (all.isEmpty()) "Nothing in the library yet." else "No matches — adjust the search or filters.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = IrisColors.MutedForeground,
                )
                else -> LazyVerticalGrid(
                    // Denser than the home shelves: more columns, smaller
                    // posters so a big library fits more per screenful.
                    columns = GridCells.Adaptive(minSize = 116.dp),
                    state = gridState,
                    modifier = Modifier.fillMaxSize(),
                    horizontalArrangement = Arrangement.spacedBy(Spacing.md),
                    verticalArrangement = Arrangement.spacedBy(Spacing.lg),
                    contentPadding = PaddingValues(vertical = Spacing.sm),
                ) {
                    itemsIndexed(visible, key = { _, it -> it.id }) { index, c ->
                        LibraryGridCard(
                            container = container,
                            collection = c,
                            // Remember which card we leave from so Back can
                            // restore focus to it.
                            onClick = {
                                lastOpenedId = c.id.toString()
                                onOpenCollection(c.id.toString())
                            },
                            modifier = if (index == targetIndex) {
                                Modifier.focusRequester(restoreFocus)
                            } else {
                                Modifier
                            },
                        )
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun FilterChip(label: String, selected: Boolean, onClick: () -> Unit) {
    val pill = RoundedCornerShape(Radius.pill)
    Surface(
        onClick = onClick,
        shape = ClickableSurfaceDefaults.shape(shape = pill),
        scale = ClickableSurfaceDefaults.scale(focusedScale = Focus.controlScale),
        colors = ClickableSurfaceDefaults.colors(
            containerColor = if (selected) IrisColors.Elev2 else IrisColors.Overlay06,
            contentColor = if (selected) IrisColors.Foreground else IrisColors.MutedForeground,
            focusedContainerColor = if (selected) IrisColors.Elev2 else IrisColors.Overlay12,
            focusedContentColor = IrisColors.Foreground,
        ),
        border = ClickableSurfaceDefaults.border(
            border = Border.None,
            focusedBorder = Border(BorderStroke(Focus.ring, IrisColors.Brand), shape = pill),
        ),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 7.dp),
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun LibraryGridCard(
    container: AppContainer,
    collection: CollectionListItem,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var meta by remember(collection.tmdbId) { mutableStateOf<MediaMetadata?>(null) }
    LaunchedEffect(collection.tmdbId, collection.kind.value) {
        val id = collection.tmdbId ?: return@LaunchedEffect
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        meta = runCatching { container.apiFor(url).tmdbMetadata(id, collection.kind.value) }.getOrNull()
    }
    val poster = tmdbPosterUrl(meta?.posterPath, "w342")
    val title = prettify(collection.displayTitle)
    val subtitle = buildString {
        if (collection.kind == MediaKind.tv && collection.episodeCount > 0) {
            append("${collection.episodeCount} ep")
        } else {
            append(formatBytesLib(collection.totalSizeBytes))
        }
        if (collection.torrentCount > 1) append(" · ${collection.torrentCount}×")
    }
    val shape = RoundedCornerShape(Radius.poster)

    Card(
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        shape = CardDefaults.shape(shape = shape),
        // Gentle pop only — the grid is dense, a big scale clips at the edges.
        scale = CardDefaults.scale(focusedScale = 1.03f),
        colors = CardDefaults.colors(containerColor = IrisColors.Card),
        border = CardDefaults.border(
            focusedBorder = Border(BorderStroke(Focus.ring, IrisColors.Brand), shape = shape),
        ),
    ) {
        Column {
            Box(Modifier.fillMaxWidth().aspectRatio(2f / 3f)) {
                if (poster != null) {
                    AsyncImage(
                        model = poster,
                        contentDescription = title,
                        modifier = Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    Box(Modifier.fillMaxSize().background(irisPosterPlaceholder()))
                    Box(
                        Modifier.fillMaxSize().padding(12.dp),
                        contentAlignment = Alignment.BottomStart,
                    ) {
                        Text(
                            title,
                            style = MaterialTheme.typography.headlineSmall,
                            color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.92f),
                            maxLines = 3,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontMono),
                    color = IrisColors.FgDim,
                )
            }
        }
    }
}

private fun prettify(raw: String): String =
    raw.substringBeforeLast('.', raw).replace('.', ' ').replace('_', ' ').trim()

private fun formatBytesLib(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0f MB", mb)
    return "$b B"
}
