package studio.kahn.iris.tv.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.BorderStroke
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.MediaKind
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.AvailableEpisodeEntry
import studio.kahn.iris.tv.data.CollectionDetail
import studio.kahn.iris.tv.data.EpisodeEntry
import studio.kahn.iris.tv.data.FileEntry
import studio.kahn.iris.tv.data.SeasonPackEntry
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.LanguageBadge
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing

private val VIDEO_EXTS_C = listOf(
    ".mkv", ".mp4", ".webm", ".m4v", ".avi", ".mov", ".ts", ".mts", ".m2ts", ".wmv",
)

/**
 * Unified TV / movie collection screen — the only "what does my
 * library hold for this show" surface post-0.4. Mirrors the web's
 * `/collection/:id`:
 *
 *   * Server-provided hero (poster + backdrop now ship inside
 *     `CollectionDetail`, no separate `tmdbMetadata` round-trip).
 *   * TV-kind: merged episode list — on-disk episodes (Play) AND
 *     indexer offers (Grab & Play / Prepare). Each available row
 *     carries a language badge so the household's anglophone +
 *     francophone users pick from the variant they want.
 *   * Movie / SCENE-unparseable TV: raw file list fallback so the
 *     user can still launch playback.
 *
 * The retired `SeriesScreen` route forwards here; the Home shelf's
 * Watchlist tile clicks land here directly.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun CollectionScreen(
    container: AppContainer,
    collectionId: String,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onBack: () -> Unit,
) {
    val layout = LocalTvLayout.current
    val scope = rememberCoroutineScope()
    var detail by remember(collectionId) { mutableStateOf<CollectionDetail?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var selectedSeason by rememberSaveable(collectionId) { mutableIntStateOf(-1) }

    suspend fun reload() {
        val url = container.sessionStore.serverUrl.first() ?: run {
            error = "Not signed in"; return
        }
        val api = container.apiFor(url)
        runCatching { api.collectionDetail(collectionId) }
            .onSuccess { detail = it; error = null }
            .onFailure { error = it.message ?: "Failed to load collection" }
    }

    LaunchedEffect(collectionId) { reload() }

    val d = detail
    if (d == null) {
        LoadingOrError(error = error, onBack = onBack)
        return
    }

    // Merge on-disk + indexer-cached episodes for TV. Available
    // entries carry a language tag so the same (S, E) can render as
    // FR + EN side by side; downloaded entries get one row regardless.
    val merged = remember(d) { mergeEpisodes(d.episodes, d.availableEpisodes.orEmpty()) }
    // Fleuve anime (One Piece): one flat absolute-numbered list, no
    // season tabs. Derived server-side, so a season-cut anime keeps the
    // seasonal layout below.
    val isAbsolute = d.numbering == "absolute"
    val absoluteRows = remember(d) { mergeEpisodesAbsolute(d.episodes, d.availableEpisodes.orEmpty()) }
    // Seasons that have either episodes OR a pack offer — a brand
    // new follow whose only signal is a pack still gets its season
    // tab so the user has a "Grab full Season N" affordance.
    val seasons = remember(merged, d.seasonPacks) {
        val map = sortedMapOf<Int, MutableList<MergedEpisode>>()
        for (row in merged) {
            map.getOrPut(row.season.toInt()) { mutableListOf() }.add(row)
        }
        for (p in d.seasonPacks.orEmpty()) {
            map.getOrPut(p.season.toInt()) { mutableListOf() }
        }
        map.mapValues { it.value.toList() }.toSortedMap()
    }
    if (selectedSeason == -1 && seasons.isNotEmpty()) {
        selectedSeason = seasons.keys.first()
    }
    val activeSeason = selectedSeason

    // An absolute (fleuve) list can be 1000+ episodes — landing focus on
    // episode 1 would mean D-padding to the bottom forever. Land on the
    // last episode the household ALREADY HAS on disk (where the user is in
    // their watch-through), not the newest available. Fall back to the
    // latest row only when nothing is downloaded yet.
    val listState = rememberLazyListState()
    val focusTarget = remember(collectionId) { FocusRequester() }
    // D-pad Up from the first row below the season tabs (a pack banner or
    // the first episode) must land on the active season pill — without
    // this, Compose's default spatial search skips the LazyRow entirely
    // and jumps straight to the hero's Back button.
    val seasonTabsFocus = remember(collectionId) { FocusRequester() }
    val focusRowIdx = remember(absoluteRows) {
        absoluteRows
            .indexOfLast { row -> row.variants.any { it is EpisodeVariant.Downloaded } }
            .let { if (it >= 0) it else absoluteRows.lastIndex }
    }
    // Keyed on (collection, the target row) so it fires once per load and
    // again only if the owned-up-to point actually moves (a new grab).
    LaunchedEffect(collectionId, isAbsolute, focusRowIdx) {
        if (isAbsolute && focusRowIdx >= 0) {
            // hero is item 0, so episode i sits at LazyColumn index 1 + i.
            listState.scrollToItem(1 + focusRowIdx)
            // The scrolled-in row's focus node may attach a frame or two
            // later — retry across a few frames for robustness.
            repeat(6) {
                withFrameNanos { }
                if (runCatching { focusTarget.requestFocus() }.isSuccess) return@LaunchedEffect
            }
        }
    }

    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background),
    ) {
        item(key = "hero") {
            CollectionHero(detail = d, onBack = onBack)
        }

        if (d.kind == MediaKind.tv && isAbsolute && absoluteRows.isNotEmpty()) {
            // Flat absolute list — no season tabs, no packs (a fleuve
            // anime numbers continuously; "Season N" packs don't apply).
            itemsIndexed(
                absoluteRows,
                key = { _, it -> it.absolute?.let { a -> "abs:$a" } ?: "se:${it.season}:${it.episode}" },
            ) { index, ep ->
                Box(
                    Modifier
                        .padding(
                            horizontal = layout.gutterHorizontal,
                            vertical = Spacing.xs,
                        )
                        // Focus target for the "land on last owned episode"
                        // effect — focusGroup forwards the request to the
                        // row's first chip.
                        .then(
                            if (index == focusRowIdx) {
                                Modifier.focusRequester(focusTarget).focusGroup()
                            } else {
                                Modifier
                            },
                        ),
                ) {
                    EpisodeRow(
                        ep = ep,
                        onPlay = onPickFile,
                        onGrabVariant = { variant ->
                            scope.launch {
                                doGrabVariant(
                                    container,
                                    collectionId,
                                    ep.season,
                                    ep.episode,
                                    variant,
                                    onPickFile,
                                )
                                reload()
                            }
                        },
                    )
                }
            }
        } else if (d.kind == MediaKind.tv && (merged.isNotEmpty() || d.seasonPacks.orEmpty().isNotEmpty())) {
            if (seasons.size > 1) {
                item(key = "season-tabs") {
                    Box(Modifier.padding(horizontal = layout.gutterHorizontal, vertical = Spacing.md)) {
                        SeasonTabs(
                            seasons = seasons.keys.toList(),
                            value = activeSeason,
                            onChange = { selectedSeason = it },
                            focusRequester = seasonTabsFocus,
                        )
                    }
                }
            }

            val currentPacks = d.seasonPacks.orEmpty().filter { it.season.toInt() == activeSeason }
            // Up-navigation from the first row below the tabs must land back
            // on the season pill — only wire it on row index 0, and only
            // when there's actually a tab row above to land on.
            val firstRowGetsUpFocus = seasons.size > 1
            if (currentPacks.isNotEmpty()) {
                itemsIndexed(
                    currentPacks,
                    key = { _, it -> "pack:${it.season}:${it.language ?: "_"}:${it.indexerTorrentId}" },
                ) { index, pack ->
                    Box(
                        Modifier
                            .padding(
                                horizontal = layout.gutterHorizontal,
                                vertical = Spacing.xs,
                            )
                            .then(
                                if (index == 0 && firstRowGetsUpFocus) {
                                    Modifier.focusProperties { up = seasonTabsFocus }
                                } else {
                                    Modifier
                                },
                            ),
                    ) {
                        SeasonPackBanner(
                            pack = pack,
                            onGrab = {
                                scope.launch {
                                    doGrabPack(container, collectionId, pack, autoPlay = true, onPlay = onPickFile)
                                    reload()
                                }
                            },
                            onPrepare = {
                                scope.launch {
                                    doGrabPack(container, collectionId, pack, autoPlay = false, onPlay = onPickFile)
                                    reload()
                                }
                            },
                        )
                    }
                }
            }

            val currentRows = seasons[activeSeason].orEmpty()
            itemsIndexed(currentRows, key = { _, it -> "${it.season}:${it.episode}" }) { index, ep ->
                Box(
                    Modifier
                        .padding(
                            horizontal = layout.gutterHorizontal,
                            vertical = Spacing.xs,
                        )
                        .then(
                            if (index == 0 && currentPacks.isEmpty() && firstRowGetsUpFocus) {
                                Modifier.focusProperties { up = seasonTabsFocus }
                            } else {
                                Modifier
                            },
                        ),
                ) {
                    EpisodeRow(
                        ep = ep,
                        onPlay = onPickFile,
                        onGrabVariant = { variant ->
                            scope.launch {
                                doGrabVariant(
                                    container,
                                    collectionId,
                                    ep.season,
                                    ep.episode,
                                    variant,
                                    onPickFile,
                                )
                                reload()
                            }
                        },
                    )
                }
            }
        } else {
            // Movie / unparsed-TV fallback. Server already sorts files
            // SCENE-aware inside the snapshot, so no client-side reorder
            // is needed.
            item(key = "files-header") {
                Eyebrow(
                    "Files",
                    modifier = Modifier.padding(
                        horizontal = layout.gutterHorizontal,
                        vertical = Spacing.md,
                    ),
                )
            }
            val files: List<Pair<TorrentView, FileEntry>> = d.torrents.flatMap { t ->
                t.files
                    .filter { f -> VIDEO_EXTS_C.any { f.path.endsWith(it, ignoreCase = true) } }
                    .map { f -> t to f }
            }
            items(files, key = { (t, f) -> "${t.infohash}:${f.index}" }) { (t, f) ->
                Box(
                    Modifier.padding(
                        horizontal = layout.gutterHorizontal,
                        vertical = Spacing.xs,
                    ),
                ) {
                    FileRow(file = f, onClick = { onPickFile(t.infohash, f.index) })
                }
            }
        }

        item(key = "trailing") {
            Box(Modifier.padding(vertical = Spacing.xl))
        }
    }
}

// ============================================================================
// Episode merge model — one row per (season, episode), variants inside
// (mirrors web's MergedEpisode shape)
// ============================================================================

private data class MergedEpisode(
    val season: Long,
    val episode: Long,
    /** Absolute episode number for fleuve anime — set only in the
     *  absolute-numbering layout. When non-null the row renders as
     *  "Episode N"; `season`/`episode` still carry the fansub
     *  coordinate used for the grab call. */
    val absolute: Long? = null,
    val variants: List<EpisodeVariant>,
)

private sealed class EpisodeVariant {
    abstract val language: String?

    /** A release that's already on disk — clicking the chip plays it. */
    data class Downloaded(
        val infohash: String,
        val fileIdx: Int,
        val watched: Boolean,
        override val language: String?,
    ) : EpisodeVariant()

    /** A cached indexer offer — clicking the chip grabs the matching
     *  language (server enforces a strict-match grab). */
    data class Available(
        val quality: String?,
        val seeders: Long?,
        val sizeBytes: Long?,
        override val language: String?,
    ) : EpisodeVariant()
}

private fun mergeEpisodes(
    onDisk: List<EpisodeEntry>,
    available: List<AvailableEpisodeEntry>,
): List<MergedEpisode> {
    // Group both downloaded and available entries under the same
    // (season, episode) key. Server already filters available rows
    // whose language is covered by an owned release — anything that
    // arrives here is a genuine additional variant the user might
    // want to grab. episode == 0 is the season-pack sentinel and
    // stays out of the per-episode grid.
    val buckets = linkedMapOf<Pair<Long, Long>, MutableList<EpisodeVariant>>()
    val ensure = { season: Long, episode: Long ->
        buckets.getOrPut(season to episode) { mutableListOf() }
    }
    for (d in onDisk) {
        if (d.episode == 0L) continue
        ensure(d.season, d.episode).add(
            EpisodeVariant.Downloaded(
                infohash = d.infohash,
                fileIdx = d.fileIdx.toInt(),
                watched = d.watched,
                language = d.language,
            ),
        )
    }
    for (a in available) {
        if (a.episode == 0L) continue
        ensure(a.season, a.episode).add(
            EpisodeVariant.Available(
                quality = a.quality,
                seeders = a.seeders,
                sizeBytes = a.sizeBytes,
                language = a.language,
            ),
        )
    }
    // Variant order inside a row: downloaded first (the natural
    // "Play" primary), then available sorted by language for
    // predictable adjacency.
    return buckets
        .map { (key, variants) ->
            val sorted = variants.sortedWith(
                compareBy(
                    { if (it is EpisodeVariant.Downloaded) 0 else 1 },
                    { it.language ?: "" },
                ),
            )
            MergedEpisode(season = key.first, episode = key.second, variants = sorted)
        }
        .sortedWith(compareBy({ it.season }, { it.episode }))
}

/** Absolute-numbering merge for fleuve anime (One Piece): group by the
 *  absolute episode number (falling back to `episode`, since a fansub
 *  `S01E1156` stores 1156 there too) into one flat ordered list — no
 *  seasons. Each row keeps the underlying (season, episode) for the
 *  grab call; `absolute` drives the "Episode N" label. Mirrors the
 *  web client's `mergeEpisodesAbsolute`. */
private fun mergeEpisodesAbsolute(
    onDisk: List<EpisodeEntry>,
    available: List<AvailableEpisodeEntry>,
): List<MergedEpisode> {
    // A long-running anime ships fleuve fansubs (`S01E1156`, absolute
    // known) AND season-cut releases (`S23E07`, no derivable absolute)
    // at once. Season-cut entries have no valid position on the absolute
    // axis, so they must NOT be folded in under their raw `episode` (that
    // aliased unrelated cuts onto a bogus "Episode 1..7"). Owned episodes
    // always appear (never hide what's on disk) — by absolute when known,
    // else by their (season, episode); available offers appear only when
    // they carry an absolute number. Mirrors web `mergeEpisodesAbsolute`.
    data class Row(val season: Long, val episode: Long, val absolute: Long?, val variants: MutableList<EpisodeVariant>)
    val buckets = linkedMapOf<String, Row>()
    val ensure = { abs: Long?, season: Long, episode: Long ->
        val key = if (abs != null) "a:$abs" else "s:$season:$episode"
        buckets.getOrPut(key) { Row(season, episode, abs, mutableListOf()) }
    }
    for (d in onDisk) {
        if (d.episode == 0L) continue
        ensure(d.absoluteEpisode, d.season, d.episode).variants.add(
            EpisodeVariant.Downloaded(
                infohash = d.infohash,
                fileIdx = d.fileIdx.toInt(),
                watched = d.watched,
                language = d.language,
            ),
        )
    }
    for (a in available) {
        if (a.episode == 0L) continue
        // Skip season-cut offers with no absolute — unplaceable here.
        val abs = a.absoluteEpisode ?: continue
        ensure(abs, a.season, a.episode).variants.add(
            EpisodeVariant.Available(
                quality = a.quality,
                seeders = a.seeders,
                sizeBytes = a.sizeBytes,
                language = a.language,
            ),
        )
    }
    return buckets.values
        .map { row ->
            val sorted = row.variants.sortedWith(
                compareBy(
                    { if (it is EpisodeVariant.Downloaded) 0 else 1 },
                    { it.language ?: "" },
                ),
            )
            MergedEpisode(season = row.season, episode = row.episode, absolute = row.absolute, variants = sorted)
        }
        // Absolute-numbered rows first (ascending); owned-without-absolute trail.
        .sortedWith(
            compareBy({ it.absolute == null }, { it.absolute ?: Long.MAX_VALUE }, { it.season }, { it.episode }),
        )
}

private suspend fun doGrabVariant(
    container: AppContainer,
    collectionId: String,
    season: Long,
    episode: Long,
    variant: EpisodeVariant.Available,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
) {
    val url = container.sessionStore.serverUrl.first() ?: return
    val api = container.apiFor(url)
    val res = withContext(Dispatchers.IO) {
        runCatching {
            api.grabCollectionEpisode(
                id = collectionId,
                season = season.toInt(),
                episode = episode.toInt(),
                language = variant.language,
            )
        }.getOrNull()
    } ?: return
    // The grab is always play-on-success on TV — the alternate
    // "Prepare" web button doesn't have a clean D-pad equivalent
    // and the user typically opens an episode to watch it.
    onPlay(res.infohash, res.fileIdx.toInt())
}

/// Grab a full season pack. Calls the same per-episode endpoint
/// with `episode = 1` — the backend falls back to the cached pack
/// (no E01 singleton expected for a pack-only season) and resolves
/// the pack's E01 file inside the snapshot. Once collection_assign
/// runs on the ingest, episode_files rows materialise for every
/// leaf, so subsequent visits see the whole season as "downloaded".
private suspend fun doGrabPack(
    container: AppContainer,
    collectionId: String,
    pack: SeasonPackEntry,
    autoPlay: Boolean,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
) {
    val url = container.sessionStore.serverUrl.first() ?: return
    val api = container.apiFor(url)
    val res = withContext(Dispatchers.IO) {
        runCatching {
            api.grabCollectionEpisode(
                id = collectionId,
                season = pack.season.toInt(),
                episode = 1,
                language = pack.language,
            )
        }.getOrNull()
    } ?: return
    if (autoPlay) {
        onPlay(res.infohash, res.fileIdx.toInt())
    }
}

// ============================================================================
// Hero
// ============================================================================

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun CollectionHero(
    detail: CollectionDetail,
    onBack: () -> Unit,
) {
    val layout = LocalTvLayout.current
    val backdrop = tmdbBackdropUrl(detail.backdropPath, "w1280")
    val poster = tmdbPosterUrl(detail.posterPath, "w342")
    Box(Modifier.fillMaxWidth().aspectRatio(layout.heroAspect)) {
        if (backdrop != null) {
            AsyncImage(
                model = backdrop,
                contentDescription = detail.displayTitle,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
            Box(
                Modifier.fillMaxSize().background(
                    Brush.verticalGradient(
                        0.4f to Color.Transparent,
                        1f to Color.Black.copy(alpha = 0.88f),
                    ),
                ),
            )
        } else {
            // Tinted gradient fallback — same aesthetic as the
            // web's empty-poster card, never a flat black void.
            Box(
                Modifier.fillMaxSize().background(
                    Brush.verticalGradient(
                        colors = listOf(
                            MaterialTheme.colorScheme.primary.copy(alpha = 0.30f),
                            MaterialTheme.colorScheme.background,
                        ),
                    ),
                ),
            )
        }
        Row(
            Modifier
                .align(Alignment.BottomStart)
                .padding(
                    start = layout.gutterHorizontal,
                    end = layout.gutterHorizontal,
                    bottom = Spacing.lg,
                )
                .fillMaxWidth(),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(Spacing.xl),
        ) {
            if (poster != null) {
                AsyncImage(
                    model = poster,
                    contentDescription = null,
                    modifier = Modifier.width(120.dp).aspectRatio(2f / 3f),
                    contentScale = ContentScale.Crop,
                )
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Spacing.xs)) {
                Text(
                    detail.displayTitle,
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                val subtitle = buildString {
                    append(if (detail.kind == MediaKind.tv) "Series" else "Movie")
                    append(" · ")
                    append(detail.torrents.size)
                    append(" torrent")
                    if (detail.torrents.size > 1) append("s")
                    if (detail.kind == MediaKind.tv && detail.episodes.isNotEmpty()) {
                        append(" · ${detail.episodes.size} episodes")
                    }
                }
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if ((detail.hasNewSinceLastVisit ?: 0) > 0) {
                    Surface(
                        shape = RoundedCornerShape(4.dp),
                        colors = SurfaceDefaults.colors(
                            containerColor = MaterialTheme.colorScheme.primary.copy(alpha = 0.85f),
                        ),
                    ) {
                        Text(
                            "${detail.hasNewSinceLastVisit} new since your last visit",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onPrimary,
                            modifier = Modifier.padding(horizontal = Spacing.sm, vertical = 2.dp),
                        )
                    }
                }
            }
            IrisButton("← Back", onBack, variant = IrisButtonVariant.Ghost)
        }
    }
}

// ============================================================================
// Season tabs + Episode rows
// ============================================================================

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SeasonTabs(
    seasons: List<Int>,
    value: Int,
    onChange: (Int) -> Unit,
    focusRequester: FocusRequester? = null,
) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
        items(seasons) { s ->
            val selected = s == value
            // ClickableSurfaceDefaults.colors has FOUR slots — the
            // resting `containerColor` we already set, but ALSO a
            // `focusedContainerColor` that defaults to the theme's
            // pale-on-light fallback. Without overriding it the
            // unselected chip turned white-on-white the moment the
            // D-pad landed on it; fix by giving every state an
            // explicit colour. Same for `contentColor` so the text
            // stays readable on every surface.
            val pill = RoundedCornerShape(Radius.pill)
            Surface(
                onClick = { onChange(s) },
                modifier = if (selected && focusRequester != null) {
                    Modifier.focusRequester(focusRequester)
                } else {
                    Modifier
                },
                shape = ClickableSurfaceDefaults.shape(shape = pill),
                // Disable the default focus scale — the tabs sit in a
                // dense LazyRow and a 1.1× pop on focus shoves
                // neighbours around. Focus reads as the brand ring + glow.
                scale = ClickableSurfaceDefaults.scale(focusedScale = 1f),
                colors = ClickableSurfaceDefaults.colors(
                    containerColor = if (selected) IrisColors.Elev2 else IrisColors.Overlay06,
                    focusedContainerColor = if (selected) IrisColors.Elev2 else IrisColors.Overlay12,
                    contentColor = if (selected) IrisColors.Foreground else IrisColors.MutedForeground,
                    focusedContentColor = IrisColors.Foreground,
                ),
                border = ClickableSurfaceDefaults.border(
                    border = Border.None,
                    focusedBorder = Border(BorderStroke(Focus.ring, IrisColors.Brand), shape = pill),
                ),
            ) {
                Text(
                    "Season $s",
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EpisodeRow(
    ep: MergedEpisode,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onGrabVariant: (EpisodeVariant.Available) -> Unit,
) {
    val anyWatched = ep.variants.any { it is EpisodeVariant.Downloaded && it.watched }
    // Layout mirrors `SeasonPackBanner` exactly (info column on the
    // left with weight=1f, action buttons on the right inside a Row
    // — fixed height container). The previous Column-of-Rows shape
    // broke D-pad focus traversal on Compose-TV: arrivals from the
    // LazyColumn didn't reach the chips below the header line.
    // Same pattern, same focus behaviour the user already confirmed
    // works for pack banners.
    Surface(
        modifier = Modifier.fillMaxWidth().height(72.dp),
        shape = RoundedCornerShape(Radius.button),
        colors = SurfaceDefaults.colors(
            containerColor = IrisColors.Overlay06,
            contentColor = MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Row(
            Modifier.fillMaxSize().padding(horizontal = Spacing.lg, vertical = Spacing.md),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
        ) {
            Row(
                modifier = Modifier.weight(1f),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Spacing.md),
            ) {
                Text(
                    // Fleuve anime rows show "Episode 1156"; seasonal
                    // rows keep the SxxExx label.
                    ep.absolute?.let { "Episode %d".format(it) }
                        ?: "S%02dE%02d".format(ep.season, ep.episode),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                if (anyWatched) {
                    Surface(
                        shape = RoundedCornerShape(4.dp),
                        colors = SurfaceDefaults.colors(
                            containerColor = Color(0xFF6B7280).copy(alpha = 0.85f),
                        ),
                    ) {
                        Text(
                            "watched",
                            style = MaterialTheme.typography.labelSmall,
                            color = Color.White,
                            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                        )
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                ep.variants.forEach { v ->
                    VariantChip(variant = v, onPlay = onPlay, onGrab = onGrabVariant)
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun VariantChip(
    variant: EpisodeVariant,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onGrab: (EpisodeVariant.Available) -> Unit,
) {
    when (variant) {
        is EpisodeVariant.Downloaded -> {
            val chipShape = RoundedCornerShape(Radius.button)
            Button(
                onClick = { onPlay(variant.infohash, variant.fileIdx) },
                shape = ButtonDefaults.shape(shape = chipShape),
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp),
                // Disable the default focused-scale pop — the chips
                // sit in a dense row, a 1.1× zoom pushes neighbours
                // off-screen on every D-pad move. Focus = brand ring + glow.
                scale = ButtonDefaults.scale(focusedScale = 1f),
                border = ButtonDefaults.border(
                    focusedBorder = Border(BorderStroke(Focus.ring, IrisColors.Brand), shape = chipShape),
                ),
            ) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    LanguageBadge(language = variant.language)
                    Text(
                        if (variant.watched) "Replay" else "Play",
                        style = MaterialTheme.typography.labelMedium,
                    )
                }
            }
        }
        is EpisodeVariant.Available -> {
            // Same `Button` shape as the Downloaded chip so D-pad
            // traversal is uniform across the row. Mixing a
            // `Surface(onClick=…)` with a `Button` in the same
            // Row used to block focus from descending onto either
            // chip — Compose-TV's focus engine treats the two as
            // separate focus contexts.
            val meta = listOfNotNull(
                variant.quality?.takeIf { it.isNotBlank() },
                variant.seeders?.let { "${it}↑" },
                variant.sizeBytes?.let { formatFileSize(it) },
            ).joinToString(" · ")
            val grabShape = RoundedCornerShape(Radius.button)
            Button(
                onClick = { onGrab(variant) },
                shape = ButtonDefaults.shape(shape = grabShape),
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 6.dp),
                scale = ButtonDefaults.scale(focusedScale = 1f),
                colors = ButtonDefaults.colors(
                    // Emerald tone for "available" so it visually
                    // reads different from the primary "Play"
                    // chip even though the focus mechanics match.
                    containerColor = IrisColors.Success.copy(alpha = 0.20f),
                    focusedContainerColor = IrisColors.Success.copy(alpha = 0.55f),
                    contentColor = MaterialTheme.colorScheme.onSurface,
                    focusedContentColor = Color.White,
                ),
                border = ButtonDefaults.border(
                    focusedBorder = Border(BorderStroke(Focus.ring, IrisColors.Brand), shape = grabShape),
                ),
            ) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    LanguageBadge(language = variant.language)
                    Text(
                        if (meta.isEmpty()) "Grab" else "Grab · $meta",
                        style = MaterialTheme.typography.labelMedium,
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SeasonPackBanner(
    pack: SeasonPackEntry,
    onGrab: () -> Unit,
    onPrepare: () -> Unit,
) {
    // Non-clickable container — the Prepare / Grab & play buttons
    // inside need to be reachable by the D-pad. A clickable Card
    // would grab focus first and the user could never land on the
    // inner buttons.
    Surface(
        modifier = Modifier.fillMaxWidth().height(88.dp),
        shape = RoundedCornerShape(12.dp),
        colors = SurfaceDefaults.colors(
            containerColor = Color(0xFF10B981).copy(alpha = 0.18f),
            contentColor = MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Row(
            Modifier.fillMaxSize().padding(horizontal = Spacing.lg, vertical = Spacing.md),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(Spacing.sm),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Surface(
                        shape = RoundedCornerShape(4.dp),
                        colors = SurfaceDefaults.colors(
                            containerColor = Color(0xFF10B981).copy(alpha = 0.85f),
                        ),
                    ) {
                        Text(
                            "SEASON PACK",
                            style = MaterialTheme.typography.labelSmall,
                            color = Color.White,
                            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                        )
                    }
                    Text(
                        "Season ${pack.season} · full pack",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    LanguageBadge(language = pack.language)
                }
                val meta = listOfNotNull(
                    pack.quality?.takeIf { it.isNotBlank() },
                    pack.seeders?.let { "$it seeders" },
                    pack.sizeBytes?.let { formatFileSize(it) },
                    "via ${pack.indexerProvider}",
                ).joinToString(" · ")
                if (meta.isNotEmpty()) {
                    Text(
                        meta,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(Spacing.sm)) {
                IrisButton("Prepare", onPrepare, variant = IrisButtonVariant.Ghost, focusedScale = 1f)
                IrisButton("Grab & play", onGrab, focusedScale = 1f)
            }
        }
    }
}


// ============================================================================
// File fallback (movies / SCENE-unparseable TV)
// ============================================================================

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun FileRow(file: FileEntry, onClick: () -> Unit) {
    val rowShape = RoundedCornerShape(Radius.button)
    Card(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().height(64.dp),
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
            Modifier.fillMaxSize().padding(horizontal = Spacing.lg, vertical = Spacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Spacing.lg),
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    file.path.substringAfterLast('/'),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                )
                Text(
                    formatFileSize(file.sizeBytes),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                "▶ Play",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

// ============================================================================
// Loading / error shell
// ============================================================================

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun LoadingOrError(error: String?, onBack: () -> Unit) {
    val layout = LocalTvLayout.current
    Box(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .padding(layout.gutterHorizontal),
        contentAlignment = Alignment.Center,
    ) {
        if (error != null) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Spacing.md),
            ) {
                Text(error, color = MaterialTheme.colorScheme.error)
                IrisButton("Back", onBack, variant = IrisButtonVariant.Ghost)
            }
        } else {
            Text("Loading collection…", color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

private fun formatFileSize(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0f MB", mb)
    return "$b B"
}
