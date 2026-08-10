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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
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
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Spacing
import studio.kahn.iris.tv.ui.components.touchClick

private val VIDEO_EXTS = listOf(
    ".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv",
)

// Fully-opaque surfaces sourced from the shared design tokens so the
// seedbox view reads as the same product as every other screen.
private val CardBg = IrisColors.Card
private val PanelBg = IrisColors.Elev2
private val Accent = IrisColors.Brand
private val Good = IrisColors.Success
private val Danger = IrisColors.Destructive

/**
 * Seedbox / raw-torrents management view. Reached from the Home top
 * bar; the family-friendly Library shelf is untouched.
 *
 * TV interaction model kept deliberately flat: a fixed compact top bar
 * (title + live summary + top-right Back, never scrolls) over a list
 * of torrent cards. Every card's actions — Play / per-file Play /
 * Delete — are **always-visible focusable buttons** (D-pad LEFT/RIGHT
 * within a card, UP/DOWN between cards). No accordion / nested
 * clickables: that's what made actions unreachable before.
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
    var totalDownloaded by remember { mutableStateOf(0L) }
    var error by remember { mutableStateOf<String?>(null) }
    var loadVersion by remember { mutableIntStateOf(0) }
    var deleting by remember { mutableStateOf<String?>(null) }

    suspend fun fetch(): Triple<List<TorrentView>, Long, Long>? {
        val url = container.sessionStore.serverUrl.first() ?: return null
        val res = withContext(Dispatchers.IO) {
            runCatching { container.apiFor(url).library("torrents") }.getOrNull()
        }
        val t = (res as? LibraryResponse.TorrentsWrapper)?.value ?: return null
        return Triple(t.items, t.totalUploadedBytes, t.totalDownloadedBytes ?: 0L)
    }

    LaunchedEffect(loadVersion) {
        error = null
        if (container.sessionStore.serverUrl.first() == null) {
            error = "Not signed in"
            return@LaunchedEffect
        }
        val got = runCatching { fetch() }.getOrNull()
        if (got == null) {
            if (torrents == null) error = "Failed to load torrents"
        } else {
            torrents = got.first
            totalUploaded = got.second
            totalDownloaded = got.third
        }
    }

    // Live refresh — same idiom as HomeScreen's downloading shelf; only
    // reassign on real change so a focused row doesn't recompose.
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(5_000)
            val got = runCatching { fetch() }.getOrNull() ?: continue
            if (got.first != torrents) torrents = got.first
            if (got.second != totalUploaded) totalUploaded = got.second
            if (got.third != totalDownloaded) totalDownloaded = got.third
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
                torrents = torrents?.filterNot { it.infohash == infohash }
            } catch (e: Exception) {
                error = e.message ?: "Delete failed"
            } finally {
                deleting = null
            }
        }
    }

    val list = torrents
    val lazyState = rememberLazyListState()
    val firstCardFocus = remember { FocusRequester() }
    var didInitialFocus by remember { mutableStateOf(false) }

    // Initial selection = the TOP card. Earlier attempts requested
    // focus before the LazyColumn had laid the node out, so the call
    // no-op'd and the system's spatial default (a fixed mid-list card,
    // reached from Home's top-right icon) won. Gate on the real layout:
    // wait until item 0 is actually placed, then request — once.
    LaunchedEffect(list) {
        if (didInitialFocus || list.isNullOrEmpty()) return@LaunchedEffect
        runCatching { lazyState.scrollToItem(0) }
        snapshotFlow { lazyState.layoutInfo.visibleItemsInfo.any { it.index == 0 } }
            .first { it }
        runCatching { firstCardFocus.requestFocus() }
        withFrameNanos { }
        runCatching { firstCardFocus.requestFocus() }
        didInitialFocus = true
    }

    Column(
        Modifier
            .fillMaxSize()
            .background(IrisColors.Background),
    ) {
        // ---- Fixed top bar (never scrolls) ----
        Column(
            Modifier
                .fillMaxWidth()
                .padding(
                    start = layout.gutterHorizontal,
                    end = layout.gutterHorizontal,
                    top = layout.gutterVertical,
                    bottom = Spacing.md,
                ),
            verticalArrangement = Arrangement.spacedBy(Spacing.md),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(Spacing.xs)) {
                    Text(
                        "Seedbox",
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.SemiBold,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                    Text(
                        when {
                            list == null -> "Loading torrents…"
                            list.isEmpty() -> "No torrents yet"
                            else -> "${list.size} torrent${if (list.size == 1) "" else "s"} · seeding"
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                IrisButton("← Back", onBack, variant = IrisButtonVariant.Ghost, focusedScale = 1f)
            }
            list?.let { SummaryStrip(it, totalUploaded, totalDownloaded) }
            error?.let {
                Text(it, style = MaterialTheme.typography.bodySmall, color = Danger)
            }
        }

        // ---- Scrolling torrent list ----
        if (list != null && list.isEmpty()) {
            Box(
                Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    "Nothing here. Add content from Search.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LazyColumn(
                modifier = Modifier.fillMaxWidth().weight(1f),
                state = lazyState,
                contentPadding = PaddingValues(
                    start = layout.gutterHorizontal,
                    end = layout.gutterHorizontal,
                    bottom = layout.gutterVertical,
                ),
                verticalArrangement = Arrangement.spacedBy(Spacing.sm),
            ) {
                itemsIndexed(list.orEmpty(), key = { _, it -> it.infohash }) { idx, t ->
                    TorrentCard(
                        t = t,
                        deleting = deleting == t.infohash,
                        externalFocus = if (idx == 0) firstCardFocus else null,
                        onPlayFile = onPickFile,
                        onDelete = { onDelete(t.infohash) },
                    )
                }
                item(key = "trailing") { Box(Modifier.height(Spacing.lg)) }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SummaryStrip(items: List<TorrentView>, totalUploaded: Long, totalDownloaded: Long) {
    val downBps = items.sumOf { it.downloadSpeedBps }
    val upBps = items.sumOf { it.uploadSpeedBps }
    // Lifetime / lifetime — dividing by the LIVE torrents' progress
    // overstated the ratio as soon as the GC had evicted anything.
    val ratio = if (totalDownloaded > 0) totalUploaded.toDouble() / totalDownloaded else null
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(10.dp),
        colors = SurfaceDefaults.colors(containerColor = PanelBg),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = Spacing.lg, vertical = Spacing.md),
            horizontalArrangement = Arrangement.spacedBy(Spacing.xl),
        ) {
            Stat("Seeded", formatBytes(totalUploaded), Accent)
            Stat("Ratio", ratio?.let { "%.2f".format(it) } ?: "—", if ((ratio ?: 0.0) >= 1.0) Good else null)
            Stat("Down", "↓ ${formatBytes(downBps)}/s", null)
            Stat("Up", "↑ ${formatBytes(upBps)}/s", Good)
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Stat(label: String, value: String, valueColor: Color?) {
    Column(verticalArrangement = Arrangement.spacedBy(Spacing.xs)) {
        Text(
            label.uppercase(),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.SemiBold,
            color = valueColor ?: MaterialTheme.colorScheme.onSurface,
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun TorrentCard(
    t: TorrentView,
    deleting: Boolean,
    externalFocus: FocusRequester?,
    onPlayFile: (infohash: String, fileIdx: Int) -> Unit,
    onDelete: () -> Unit,
) {
    var showFiles by remember(t.infohash) { mutableStateOf(false) }
    var confirming by remember(t.infohash) { mutableStateOf(false) }
    // Set true the moment the user opens this card's confirm flow, so
    // the confirm/cancel focus handoff only fires after a real
    // interaction (never steals focus during initial composition).
    var armed by remember(t.infohash) { mutableStateOf(false) }
    // One requester, always parked on whichever button is *leading* in
    // the current state (primary action, or Confirm while confirming).
    // The first card uses the screen-provided requester so the screen
    // can focus it once the list has actually been laid out.
    val internalFocus = remember(t.infohash) { FocusRequester() }
    val cardFocus = externalFocus ?: internalFocus
    val leadingMod = Modifier.focusRequester(cardFocus)

    // Action set swapped (Delete ⇄ Confirm/Cancel): the old focused
    // button left the tree, so re-anchor focus inside this card instead
    // of letting it escape to the previous torrent.
    LaunchedEffect(confirming) {
        if (!armed) return@LaunchedEffect
        repeat(10) {
            runCatching { cardFocus.requestFocus() }
            withFrameNanos { }
        }
    }

    val videos = remember(t.files) {
        t.files
            .filter { f -> VIDEO_EXTS.any { f.path.endsWith(it, ignoreCase = true) } }
            .sortedByDescending { it.sizeBytes }
    }
    val single = videos.size == 1
    val multi = videos.size > 1
    val pct = t.progressPct.toFloat().coerceIn(0f, 100f)
    val finished = pct >= 100f
    // Both counters are lifetime (survive restarts and regrabs) so the
    // ratio is stable from the first paint — no more inflated values
    // while a fresh session's progress catches up to a past life's seed.
    val lifetimeDownloaded = t.downloadedBytesTotal ?: 0L
    val ratio =
        if (lifetimeDownloaded > 0) t.uploadedBytesTotal.toDouble() / lifetimeDownloaded
        else null

    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = SurfaceDefaults.colors(containerColor = CardBg),
    ) {
        Column(
            Modifier.fillMaxWidth().padding(Spacing.lg),
            verticalArrangement = Arrangement.spacedBy(Spacing.sm),
        ) {
            Text(
                t.name ?: t.infohash,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Row(
                horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                StateBadge(t.state.value)
                Meta("${t.peers} peer${if (t.peers == 1) "" else "s"}")
                Meta("${formatBytes(t.progressBytes)} / ${formatBytes(t.totalSizeBytes)}")
                Meta("↓ ${formatBytes(t.downloadSpeedBps)}/s")
                Meta("↑ ${formatBytes(t.uploadSpeedBps)}/s")
                ratio?.let {
                    Text(
                        "ratio ${"%.2f".format(it)}",
                        style = MaterialTheme.typography.labelSmall,
                        color = if (it >= 1.0) Good else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            ProgressBar(pct = pct, finished = finished)
            t.error?.let {
                Text(it, style = MaterialTheme.typography.labelSmall, color = Danger)
            }

            // Actions — always visible, always focusable.
            Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                if (confirming) {
                    Button(
                        onClick = onDelete,
                        enabled = !deleting,
                        modifier = leadingMod.touchClick(enabled = !deleting, onClick = onDelete),
                        scale = ButtonDefaults.scale(focusedScale = 1f),
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(10.dp)),
                        colors = ButtonDefaults.colors(
                            containerColor = Danger,
                            contentColor = Color(0xFF1A0B0B),
                        ),
                        contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                    ) { Text(if (deleting) "Deleting…" else "Confirm") }
                    Button(
                        onClick = { confirming = false },
                        enabled = !deleting,
                        modifier = Modifier.touchClick(enabled = !deleting) { confirming = false },
                        scale = ButtonDefaults.scale(focusedScale = 1f),
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(10.dp)),
                        contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                    ) { Text("Cancel") }
                } else {
                    if (single) {
                        Button(
                            onClick = { onPlayFile(t.infohash, videos[0].index) },
                            modifier = leadingMod.touchClick { onPlayFile(t.infohash, videos[0].index) },
                            scale = ButtonDefaults.scale(focusedScale = 1f),
                            shape = ButtonDefaults.shape(shape = RoundedCornerShape(10.dp)),
                            contentPadding = PaddingValues(horizontal = 20.dp, vertical = 10.dp),
                        ) { Text("▶ Play") }
                    }
                    if (multi) {
                        Button(
                            onClick = { showFiles = !showFiles },
                            modifier = leadingMod.touchClick { showFiles = !showFiles },
                            scale = ButtonDefaults.scale(focusedScale = 1f),
                            shape = ButtonDefaults.shape(shape = RoundedCornerShape(10.dp)),
                            contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                        ) { Text(if (showFiles) "Hide files" else "▶ ${videos.size} files") }
                    }
                    Button(
                        onClick = { armed = true; confirming = true },
                        enabled = !deleting,
                        modifier = (if (single || multi) Modifier else leadingMod)
                            .touchClick(enabled = !deleting) { armed = true; confirming = true },
                        scale = ButtonDefaults.scale(focusedScale = 1f),
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(10.dp)),
                        colors = ButtonDefaults.colors(
                            containerColor = PanelBg,
                            contentColor = Danger,
                        ),
                        contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                    ) { Text("Delete") }
                }
            }

            if (showFiles && multi && !confirming) {
                Column(
                    Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(Spacing.xs),
                ) {
                    for (f in videos) {
                        Button(
                            onClick = { onPlayFile(t.infohash, f.index) },
                            modifier = Modifier
                                .fillMaxWidth()
                                .touchClick { onPlayFile(t.infohash, f.index) },
                            scale = ButtonDefaults.scale(focusedScale = 1f),
                            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                            colors = ButtonDefaults.colors(
                                containerColor = PanelBg,
                                contentColor = MaterialTheme.colorScheme.onSurface,
                            ),
                            contentPadding = PaddingValues(
                                horizontal = Spacing.lg,
                                vertical = Spacing.sm,
                            ),
                        ) {
                            Row(
                                Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(Spacing.md),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    f.path.substringAfterLast('/'),
                                    style = MaterialTheme.typography.bodySmall,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.weight(1f),
                                )
                                Text(
                                    "▶ ${formatBytes(f.sizeBytes)}",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = Accent,
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
private fun Meta(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun ProgressBar(pct: Float, finished: Boolean) {
    Box(
        Modifier
            .fillMaxWidth()
            .height(6.dp)
            .background(IrisColors.Elev2, RoundedCornerShape(3.dp)),
    ) {
        Box(
            Modifier
                .fillMaxWidth(fraction = (pct / 100f).coerceIn(0f, 1f))
                .height(6.dp)
                .background(if (finished) Good else Accent, RoundedCornerShape(3.dp)),
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun StateBadge(state: String) {
    val (label, color) = when (state) {
        "live" -> "live" to Good
        "paused" -> "paused" to IrisColors.Warn
        "error" -> "error" to Danger
        else -> "starting" to IrisColors.FgDim
    }
    Surface(
        shape = RoundedCornerShape(5.dp),
        colors = SurfaceDefaults.colors(containerColor = color),
    ) {
        Text(
            label.uppercase(),
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.SemiBold,
            color = IrisColors.OnBrand,
            modifier = Modifier.padding(horizontal = 7.dp, vertical = 2.dp),
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
