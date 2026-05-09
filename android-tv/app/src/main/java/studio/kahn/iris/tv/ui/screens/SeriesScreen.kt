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
import androidx.compose.foundation.lazy.LazyColumn
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
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.ClickableSurfaceDefaults
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.Text
import coil3.compose.AsyncImage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.AddFollowRequest
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.EpisodeItem
import studio.kahn.iris.tv.data.EpisodesResponse
import studio.kahn.iris.tv.data.FollowSummary
import studio.kahn.iris.tv.data.TmdbMetadata
import studio.kahn.iris.tv.data.tmdbBackdropUrl
import studio.kahn.iris.tv.data.tmdbPosterUrl

/**
 * Series detail screen — TV equivalent of the web /series/:tmdb_id page.
 * Hero band with backdrop + poster, season tabs, episode rows with the
 * right primary action per status (Lire / Préparer / À venir). Follow
 * toggle in the hero. No auto-grab — user always confirms.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun SeriesScreen(
    container: AppContainer,
    tmdbId: Long,
    onPickFile: (infohash: String, fileIdx: Int) -> Unit,
    onBack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var meta by remember(tmdbId) { mutableStateOf<TmdbMetadata?>(null) }
    var follow by remember(tmdbId) { mutableStateOf<FollowSummary?>(null) }
    var episodes by remember { mutableStateOf<EpisodesResponse?>(null) }
    var season by remember(tmdbId) { mutableIntStateOf(1) }
    var error by remember { mutableStateOf<String?>(null) }
    var followBusy by remember { mutableStateOf(false) }

    // Initial load: TMDB metadata + follow status. Episode list waits
    // until the user actually follows the show.
    LaunchedEffect(tmdbId) {
        error = null
        val url = container.sessionStore.serverUrl.first()
        if (url == null) {
            error = "Not signed in"
            return@LaunchedEffect
        }
        val api = container.apiFor(url)
        val (m, follows) = withContext(Dispatchers.IO) {
            val m = runCatching { api.tmdbMetadata(tmdbId) }.getOrNull()
            val follows = runCatching { api.listFollows() }.getOrDefault(emptyList())
            m to follows
        }
        meta = m
        follow = follows.firstOrNull { it.tmdbId == tmdbId }
    }

    // Episode list per season — re-fetched whenever the user switches
    // season tab OR after a successful Follow / Unfollow / Grab.
    LaunchedEffect(tmdbId, season, follow != null) {
        if (follow == null) {
            episodes = null
            return@LaunchedEffect
        }
        val url = container.sessionStore.serverUrl.first() ?: return@LaunchedEffect
        val api = container.apiFor(url)
        val res = withContext(Dispatchers.IO) {
            runCatching { api.followEpisodes(tmdbId, season) }
        }
        res.onSuccess { episodes = it }
        res.onFailure { error = it.message ?: "Failed to load episodes" }
    }

    val totalSeasons = follow?.totalSeasons ?: meta?.numberOfSeasons ?: 1
    val scroll = rememberScrollState()

    Column(
        Modifier.fillMaxSize().verticalScroll(scroll),
    ) {
        Hero(
            meta = meta,
            tmdbId = tmdbId,
            followed = follow != null,
            followBusy = followBusy,
            onToggleFollow = {
                if (followBusy) return@Hero
                followBusy = true
                scope.launch {
                    val url = container.sessionStore.serverUrl.first()
                    if (url == null) {
                        error = "Not signed in"
                        followBusy = false
                        return@launch
                    }
                    val api = container.apiFor(url)
                    try {
                        if (follow == null) {
                            follow = withContext(Dispatchers.IO) {
                                api.addFollow(
                                    AddFollowRequest(
                                        tmdbId = tmdbId,
                                        name = meta?.title,
                                        totalSeasons = meta?.numberOfSeasons,
                                    )
                                )
                            }
                        } else {
                            withContext(Dispatchers.IO) { api.removeFollow(tmdbId) }
                            follow = null
                            episodes = null
                        }
                    } catch (e: Exception) {
                        error = e.message ?: "Follow toggle failed"
                    } finally {
                        followBusy = false
                    }
                }
            },
            onBack = onBack,
        )

        Column(
            Modifier.padding(horizontal = 60.dp, vertical = 28.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp),
        ) {
            error?.let {
                Text(it, color = MaterialTheme.colorScheme.error)
            }

            if (follow == null) {
                Text(
                    "Suis cette série pour voir les épisodes attendus, ce qui est dispo et ce qui manque.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                if (totalSeasons > 1) {
                    SeasonTabs(total = totalSeasons, value = season, onChange = { season = it })
                }
                episodes?.let { eps ->
                    EpisodesList(
                        items = eps.items,
                        onPlay = onPickFile,
                        onGrab = { ep, andPlay ->
                            scope.launch {
                                val url = container.sessionStore.serverUrl.first() ?: return@launch
                                val api = container.apiFor(url)
                                try {
                                    val res = withContext(Dispatchers.IO) {
                                        api.grabEpisode(tmdbId, ep.season, ep.episode)
                                    }
                                    if (andPlay) {
                                        onPickFile(res.infohash, res.fileIdx)
                                    } else {
                                        // Re-fetch to flip the row's status badge.
                                        episodes = withContext(Dispatchers.IO) {
                                            runCatching { api.followEpisodes(tmdbId, season) }
                                                .getOrNull()
                                        }
                                    }
                                } catch (e: Exception) {
                                    error = e.message ?: "Grab failed"
                                }
                            }
                        },
                    )
                } ?: Text(
                    "Chargement des épisodes…",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun Hero(
    meta: TmdbMetadata?,
    tmdbId: Long,
    followed: Boolean,
    followBusy: Boolean,
    onToggleFollow: () -> Unit,
    onBack: () -> Unit,
) {
    val backdrop = tmdbBackdropUrl(meta?.backdropPath, "w1280")
    val poster = tmdbPosterUrl(meta?.posterPath, "w342")
    Box(
        Modifier
            .fillMaxWidth()
            .aspectRatio(16f / 5f),
    ) {
        if (backdrop != null) {
            AsyncImage(
                model = backdrop,
                contentDescription = meta?.title,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
            Box(
                Modifier
                    .fillMaxSize()
                    .background(
                        androidx.compose.ui.graphics.Brush.verticalGradient(
                            0.5f to androidx.compose.ui.graphics.Color.Transparent,
                            1f to androidx.compose.ui.graphics.Color.Black.copy(alpha = 0.85f),
                        ),
                    ),
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
                    meta?.title ?: "Chargement…",
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                meta?.overview?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 3,
                    )
                }
            }
            Column(
                horizontalAlignment = Alignment.End,
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Button(
                    onClick = onToggleFollow,
                    enabled = !followBusy,
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp),
                ) {
                    Text(if (followed) "✓  Suivi" else "♥  Suivre")
                }
                Button(
                    onClick = onBack,
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
                ) { Text("Retour") }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun SeasonTabs(total: Int, value: Int, onChange: (Int) -> Unit) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items((1..total).toList()) { s ->
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
                    "Saison $s",
                    style = MaterialTheme.typography.labelLarge,
                    color = if (selected) MaterialTheme.colorScheme.onPrimary
                    else MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
                )
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EpisodesList(
    items: List<EpisodeItem>,
    onPlay: (infohash: String, fileIdx: Int) -> Unit,
    onGrab: (EpisodeItem, /* andPlay */ Boolean) -> Unit,
) {
    if (items.isEmpty()) {
        Text(
            "TMDB n'a pas encore listé les épisodes de cette saison.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        return
    }
    // Use a regular Column rather than LazyColumn so the parent
    // verticalScroll handles the scrolling — nesting two scrollable
    // containers on TV breaks D-pad focus traversal between sections.
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
                        ep.name ?: "Épisode ${ep.episode}",
                        style = MaterialTheme.typography.bodyLarge,
                        maxLines = 1,
                    )
                    StatusBadge(ep)
                }
                ep.overview?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 2,
                    )
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ep.airDate?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    ep.runtimeMinutes?.let {
                        Text(
                            "${it} min",
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
        ep.watched -> "vu" to androidx.compose.ui.graphics.Color(0xFF6B7280)
        ep.status == "downloaded" -> "téléchargé" to androidx.compose.ui.graphics.Color(0xFF6B7280)
        ep.status == "available" -> "dispo" to androidx.compose.ui.graphics.Color(0xFF10B981)
        else -> "à venir" to androidx.compose.ui.graphics.Color(0xFF374151)
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
    when {
        ep.status == "downloaded" && ep.infohash != null && ep.fileIdx != null -> {
            Button(
                onClick = { onPlay(ep.infohash, ep.fileIdx) },
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
            ) { Text(if (ep.watched) "▶  Revoir" else "▶  Lire") }
        }
        ep.status == "available" -> {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = { onGrab(ep, false) },
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 10.dp),
                ) { Text("⤓  Préparer") }
                Button(
                    onClick = { onGrab(ep, true) },
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 18.dp, vertical = 10.dp),
                ) { Text("▶  Lire") }
            }
        }
        else -> {
            Text(
                "À venir",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
