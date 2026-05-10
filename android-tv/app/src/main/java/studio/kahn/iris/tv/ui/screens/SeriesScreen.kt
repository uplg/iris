package studio.kahn.iris.tv.ui.screens

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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
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
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.EpisodeItem
import studio.kahn.iris.tv.data.EpisodesResponse
import studio.kahn.iris.tv.data.FollowSummary
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.data.tmdbPosterUrl

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
    var selectedSeason by remember(followId) { mutableIntStateOf(-1) }

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
            val matched = list.firstOrNull { it.id == followId }
            val episodesRes = runCatching { api.followEpisodes(followId) }.getOrNull()
            matched to episodesRes
        }
        follow = f
        episodes = eps
    }

    val seasons = (episodes?.items ?: emptyList())
        .groupBy { it.season }
        .toSortedMap()
    if (selectedSeason == -1 && seasons.isNotEmpty()) {
        selectedSeason = seasons.keys.first()
    }

    val scroll = rememberScrollState()
    Column(Modifier.fillMaxSize().verticalScroll(scroll)) {
        Hero(
            follow = follow,
            unfollowBusy = unfollowBusy,
            onUnfollow = {
                if (unfollowBusy) return@Hero
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
            },
            onBack = onBack,
        )

        Column(
            Modifier.padding(horizontal = 60.dp, vertical = 28.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp),
        ) {
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }

            if (follow == null && error == null) {
                Text(
                    "Loading…",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else if (episodes == null && follow != null) {
                Text(
                    "Loading episodes…",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else if (seasons.isEmpty() && episodes != null) {
                Text(
                    "No episodes found yet. The scheduler runs every 4 h.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else if (seasons.isNotEmpty()) {
                if (seasons.size > 1) {
                    SeasonTabs(
                        seasons = seasons.keys.toList(),
                        value = selectedSeason,
                        onChange = { selectedSeason = it },
                    )
                }
                EpisodesList(
                    items = (seasons[selectedSeason] ?: emptyList())
                        .sortedBy { it.episode },
                    onPlay = onPickFile,
                    onGrab = { ep, andPlay ->
                        scope.launch {
                            val url = container.sessionStore.serverUrl.first() ?: return@launch
                            val api = container.apiFor(url)
                            try {
                                val res = withContext(Dispatchers.IO) {
                                    api.grabEpisode(followId, ep.season, ep.episode)
                                }
                                if (andPlay) {
                                    onPickFile(res.infohash, res.fileIdx)
                                } else {
                                    episodes = withContext(Dispatchers.IO) {
                                        runCatching { api.followEpisodes(followId) }.getOrNull()
                                    }
                                }
                            } catch (e: Exception) {
                                error = e.message ?: "Grab failed"
                            }
                        }
                    },
                )
            }
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
    Box(Modifier.fillMaxWidth().aspectRatio(16f / 5f)) {
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
                            MaterialTheme.colorScheme.primary.copy(alpha = 0.30f),
                            androidx.compose.ui.graphics.Color(0xFF0B0D12),
                        ),
                    ),
                ),
            )
        }
        Row(
            Modifier
                .align(Alignment.BottomStart)
                .padding(start = 60.dp, bottom = 20.dp, end = 60.dp)
                .fillMaxWidth(),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(24.dp),
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
                Button(
                    onClick = onUnfollow,
                    enabled = !unfollowBusy && follow != null,
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
                ) { Text("Unfollow") }
                Button(
                    onClick = onBack,
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
                ) { Text("Back") }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SeasonTabs(seasons: List<Int>, value: Int, onChange: (Int) -> Unit) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items(seasons) { s ->
            val selected = s == value
            Surface(
                onClick = { onChange(s) },
                shape = ClickableSurfaceDefaults.shape(shape = RoundedCornerShape(8.dp)),
                colors = ClickableSurfaceDefaults.colors(
                    containerColor = if (selected) MaterialTheme.colorScheme.primary
                    else MaterialTheme.colorScheme.surfaceVariant,
                ),
            ) {
                Text(
                    "Season $s",
                    style = MaterialTheme.typography.labelLarge,
                    color = if (selected) MaterialTheme.colorScheme.onPrimary
                    else MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun EpisodesList(
    items: List<EpisodeItem>,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onGrab: (EpisodeItem, /* andPlay */ Boolean) -> Unit,
) {
    if (items.isEmpty()) return
    // Use a regular Column rather than LazyColumn so the parent
    // verticalScroll handles the scrolling — nesting two scrollable
    // containers on TV breaks D-pad focus traversal.
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        items.forEach { ep ->
            EpisodeRow(ep = ep, onPlay = onPlay, onGrab = onGrab)
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
        shape = RoundedCornerShape(8.dp),
        colors = SurfaceDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
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
        ep.status == "downloaded" -> "downloaded" to androidx.compose.ui.graphics.Color(0xFF6B7280)
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
    if (ep.status == "downloaded" && ep.infohash != null && ep.fileIdx != null) {
        Button(
            onClick = { onPlay(ep.infohash, ep.fileIdx) },
            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
            contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
        ) { Text(if (ep.watched) "Watch again" else "Play") }
        return
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(
            onClick = { onGrab(ep, false) },
            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 10.dp),
        ) { Text("Prepare") }
        Button(
            onClick = { onGrab(ep, true) },
            shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
            contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
        ) { Text("Play") }
    }
}
