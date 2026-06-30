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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.BorderStroke
import androidx.tv.material3.Border
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
import studio.kahn.iris.tv.data.EpisodeStatus
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.EpisodeItem
import studio.kahn.iris.tv.data.EpisodesResponse
import studio.kahn.iris.tv.data.FollowSummary
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.data.tmdbPosterUrl
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.theme.Focus
import studio.kahn.iris.tv.ui.theme.IrisColors
import studio.kahn.iris.tv.ui.theme.LocalTvLayout
import studio.kahn.iris.tv.ui.theme.Radius
import studio.kahn.iris.tv.ui.theme.Spacing

/**
 * SCENE-mode series detail. Routed by follow id; episodes come from
 * the server-side union of episode_files (on disk) and
 * available_episodes (indexer cache), keyed on the follow's
 * SCENE-normalised name. No TMDB call client-side — the follow
 * summary already carries posterPath/backdropPath if the joined
 * collection is tmdb_verified.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun SeriesScreen(
    container: AppContainer,
    followId: String,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onBack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var follow by remember(followId) { mutableStateOf<FollowSummary?>(null) }
    var episodes by remember(followId) { mutableStateOf<EpisodesResponse?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var unfollowBusy by remember { mutableStateOf(false) }
    var selectedSeason by rememberSaveable(followId) { mutableIntStateOf(-1) }
    // D-pad Up from the first episode row must land on the active season
    // pill, not skip past it to the hero's Back button — see CollectionScreen
    // for the identical fix and rationale.
    val seasonTabsFocus = remember(followId) { FocusRequester() }

    LaunchedEffect(followId) {
        error = null
        val url = container.sessionStore.serverUrl.first()
        if (url == null) {
            error = "Not signed in"
            return@LaunchedEffect
        }
        val api = container.apiFor(url)
        val (f, eps) = withContext(Dispatchers.IO) {
            val list = runCatching { api.listFollows() }.getOrDefault(emptyList())
            val matched = list.firstOrNull { it.id.toString() == followId }
            val episodesRes = runCatching { api.followEpisodes(followId) }.getOrNull()
            matched to episodesRes
        }
        follow = f
        episodes = eps
    }

    val seasons = (episodes?.items ?: emptyList())
        .groupBy { it.season.toInt() }
        .toSortedMap()
    if (selectedSeason == -1 && seasons.isNotEmpty()) {
        selectedSeason = seasons.keys.first()
    }

    val layout = LocalTvLayout.current
    val onUnfollow: () -> Unit = onUnfollow@{
        if (unfollowBusy) return@onUnfollow
        unfollowBusy = true
        scope.launch {
            val url = container.sessionStore.serverUrl.first()
            if (url == null) {
                error = "Not signed in"
                unfollowBusy = false
                return@launch
            }
            val api = container.apiFor(url)
            try {
                withContext(Dispatchers.IO) { api.removeFollow(followId) }
                onBack()
            } catch (e: Exception) {
                error = e.message ?: "Unfollow failed"
            } finally {
                unfollowBusy = false
            }
        }
    }
    val onGrab: (EpisodeItem, Boolean) -> Unit = { ep, andPlay ->
        scope.launch {
            val url = container.sessionStore.serverUrl.first() ?: return@launch
            val api = container.apiFor(url)
            try {
                val res = withContext(Dispatchers.IO) {
                    api.grabEpisode(followId, ep.season.toInt(), ep.episode.toInt())
                }
                if (andPlay) {
                    onPickFile(res.infohash, res.fileIdx.toInt())
                } else {
                    episodes = withContext(Dispatchers.IO) {
                        runCatching { api.followEpisodes(followId) }.getOrNull()
                    }
                }
            } catch (e: Exception) {
                error = e.message ?: "Grab failed"
            }
        }
    }

    LazyColumn(modifier = Modifier.fillMaxSize()) {
        // Hero is a single edge-to-edge item — bypass the page gutter so
        // the backdrop bleeds to the screen edges, just like Netflix.
        item(key = "hero") {
            Hero(
                follow = follow,
                unfollowBusy = unfollowBusy,
                onUnfollow = onUnfollow,
                onBack = onBack,
            )
        }

        item(key = "status") {
            Column(
                Modifier.padding(
                    horizontal = layout.gutterHorizontal,
                    vertical = Spacing.lg,
                ),
                verticalArrangement = Arrangement.spacedBy(Spacing.md),
            ) {
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                when {
                    follow == null && error == null -> Text(
                        "Loading…",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    episodes == null && follow != null -> Text(
                        "Loading episodes…",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    seasons.isEmpty() && episodes != null -> Text(
                        "No episodes found yet. The scheduler runs every 4 h.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }

        if (seasons.size > 1) {
            item(key = "seasons") {
                Box(
                    Modifier.padding(
                        horizontal = layout.gutterHorizontal,
                        vertical = Spacing.sm,
                    ),
                ) {
                    SeasonTabs(
                        seasons = seasons.keys.toList(),
                        value = selectedSeason,
                        onChange = { selectedSeason = it },
                        focusRequester = seasonTabsFocus,
                    )
                }
            }
        }

        val visible = (seasons[selectedSeason] ?: emptyList()).sortedBy { it.episode }
        itemsIndexed(visible, key = { _, it -> "${it.season}:${it.episode}" }) { index, ep ->
            Box(
                Modifier
                    .padding(
                        horizontal = layout.gutterHorizontal,
                        vertical = Spacing.xs,
                    )
                    .then(
                        if (index == 0 && seasons.size > 1) {
                            Modifier.focusProperties { up = seasonTabsFocus }
                        } else {
                            Modifier
                        },
                    ),
            ) {
                EpisodeRow(ep = ep, onPlay = onPickFile, onGrab = onGrab)
            }
        }

        item(key = "trailing") {
            Box(Modifier.padding(vertical = Spacing.xl))
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Hero(
    follow: FollowSummary?,
    unfollowBusy: Boolean,
    onUnfollow: () -> Unit,
    onBack: () -> Unit,
) {
    val backdrop = tmdbBackdropUrl(follow?.backdropPath, "w1280")
    val poster = tmdbPosterUrl(follow?.posterPath, "w342")
    val layout = LocalTvLayout.current
    Box(Modifier.fillMaxWidth().aspectRatio(layout.heroAspect)) {
        if (backdrop != null) {
            AsyncImage(
                model = backdrop,
                contentDescription = follow?.name,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
            Box(
                Modifier.fillMaxSize().background(
                    androidx.compose.ui.graphics.Brush.verticalGradient(
                        0.5f to androidx.compose.ui.graphics.Color.Transparent,
                        1f to androidx.compose.ui.graphics.Color.Black.copy(alpha = 0.85f),
                    ),
                ),
            )
        } else {
            Box(
                Modifier.fillMaxSize().background(
                    androidx.compose.ui.graphics.Brush.verticalGradient(
                        colors = listOf(
                            IrisColors.Brand.copy(alpha = 0.30f),
                            IrisColors.BackgroundDeep,
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
            Column(
                Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    follow?.name ?: "Loading…",
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                follow?.let {
                    Text(
                        "SCENE: ${it.normalizedName}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (it.newCount > 0) {
                        Text(
                            "${it.newCount} new episode${if (it.newCount > 1) "s" else ""} since last visit",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            }
            Column(
                horizontalAlignment = Alignment.End,
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                IrisButton(
                    "Unfollow",
                    onUnfollow,
                    variant = IrisButtonVariant.Ghost,
                    enabled = !unfollowBusy && follow != null,
                )
                IrisButton("Back", onBack, variant = IrisButtonVariant.Ghost)
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SeasonTabs(
    seasons: List<Int>,
    value: Int,
    onChange: (Int) -> Unit,
    focusRequester: FocusRequester? = null,
) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items(seasons) { s ->
            val selected = s == value
            val pill = RoundedCornerShape(Radius.pill)
            Surface(
                onClick = { onChange(s) },
                modifier = if (selected && focusRequester != null) {
                    Modifier.focusRequester(focusRequester)
                } else {
                    Modifier
                },
                shape = ClickableSurfaceDefaults.shape(shape = pill),
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
    ep: EpisodeItem,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onGrab: (EpisodeItem, Boolean) -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(Radius.button),
        colors = SurfaceDefaults.colors(
            containerColor = IrisColors.Overlay06,
        ),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                "%02d".format(ep.episode),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.width(36.dp),
            )
            Column(Modifier.weight(1f)) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        "S%02dE%02d".format(ep.season, ep.episode),
                        style = MaterialTheme.typography.bodyLarge,
                        maxLines = 1,
                    )
                    StatusBadge(ep)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ep.quality?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    ep.seeders?.let {
                        Text(
                            "$it seeders",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            EpisodeAction(ep = ep, onPlay = onPlay, onGrab = onGrab)
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun StatusBadge(ep: EpisodeItem) {
    val (label, color) = when {
        ep.watched -> "watched" to androidx.compose.ui.graphics.Color(0xFF6B7280)
        ep.status == EpisodeStatus.downloaded -> "downloaded" to androidx.compose.ui.graphics.Color(0xFF6B7280)
        else -> "available" to androidx.compose.ui.graphics.Color(0xFF10B981)
    }
    Surface(
        shape = RoundedCornerShape(4.dp),
        colors = SurfaceDefaults.colors(containerColor = color.copy(alpha = 0.85f)),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = androidx.compose.ui.graphics.Color.White,
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EpisodeAction(
    ep: EpisodeItem,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onGrab: (EpisodeItem, Boolean) -> Unit,
) {
    if (ep.status == EpisodeStatus.downloaded && ep.infohash != null && ep.fileIdx != null) {
        IrisButton(
            if (ep.watched) "Watch again" else "Play",
            { onPlay(ep.infohash, ep.fileIdx.toInt()) },
            focusedScale = 1f,
        )
        return
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        IrisButton("Prepare", { onGrab(ep, false) }, variant = IrisButtonVariant.Ghost, focusedScale = 1f)
        IrisButton("Play", { onGrab(ep, true) }, focusedScale = 1f)
    }
}
