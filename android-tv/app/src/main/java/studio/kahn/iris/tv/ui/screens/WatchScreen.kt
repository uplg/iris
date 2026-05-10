// File-level opt-in: every PlayerView / Player.Listener / MediaItem method
// we touch is gated behind `@RequiresOptIn(UnstableApi)`. Per-function
// `@OptIn` doesn't propagate into AndroidView lambdas, so the `lintDebug`
// task flagged half the file. `androidx.annotation.OptIn` silences the
// Android lint analyser (`UnsafeOptInUsageError`); kotlinc does NOT
// require its own `@OptIn` here — Media3's marker is annotated with
// `androidx.annotation.RequiresOptIn`, not `kotlin.RequiresOptIn`, so a
// `@file:kotlin.OptIn(UnstableApi::class)` would emit a "has no effect"
// warning.
@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.screens

import android.net.Uri
import androidx.core.net.toUri
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.widget.TextView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.PlayerView
import androidx.tv.material3.Button
import androidx.tv.material3.ButtonDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.R
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.EpisodePoint
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.MediaProbe
import studio.kahn.iris.tv.data.PlayStatus
import studio.kahn.iris.tv.data.ProgressUpdate
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.buildMediaItem
import studio.kahn.iris.tv.data.buildPlayer
import studio.kahn.iris.tv.data.installIrisTrackNameProvider

/**
 * Full-screen Media3 PlayerView. Pre-mount we poll `/play/status` so
 * the user sees real download / remux progress instead of a silent
 * black screen, and we only construct the player once the synthetic
 * HLS master playlist is on disk (the segments themselves are produced
 * in the background and long-polled by the HTTP layer when missing).
 *
 * D-pad maps to PlayerView's built-in TV controls (play/pause, seek,
 * subtitle + audio track selection via the settings menu). Multi-audio
 * renditions are exposed natively by `HlsMediaSource` — Media3 picks
 * up `EXT-X-MEDIA TYPE=AUDIO` entries from the manifest and surfaces
 * them via the standard track-selection API.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun WatchScreen(
    container: AppContainer,
    infohash: String,
    fileIdx: Int,
    onBack: () -> Unit,
) {
    var serverUrl by remember { mutableStateOf<String?>(null) }
    var probe by remember { mutableStateOf<MediaProbe?>(null) }
    var playStatus by remember { mutableStateOf<PlayStatus?>(null) }
    var torrent by remember { mutableStateOf<TorrentView?>(null) }
    var resumePositionSec by remember { mutableStateOf(0.0) }
    var error by remember { mutableStateOf<String?>(null) }
    var probeVersion by remember { mutableStateOf(0) }

    // Probe with retry. When the user clicks Play right after ingest, the
    // file isn't on disk yet — librqbit needs a few seconds to fetch the
    // first sequential chunks. The server returns 400 "file not yet on
    // disk: …" in that window. We retry every 2s for up to ~2 min so the
    // user sees the LoadingOverlay tick down instead of bouncing straight
    // to an error screen. 401 / 404 bail immediately.
    LaunchedEffect(infohash, fileIdx, probeVersion) {
        error = null
        probe = null
        val url = container.sessionStore.serverUrl.first()
        if (url == null) {
            error = "Not signed in"
            return@LaunchedEffect
        }
        serverUrl = url
        val api: IrisApi = container.apiFor(url)
        var attempts = 0
        val maxAttempts = 60 // ~2 min at 2s each
        while (attempts < maxAttempts) {
            try {
                probe = api.probe(infohash, fileIdx)
                val progresses = runCatching { api.torrentProgress(infohash) }.getOrDefault(emptyList())
                resumePositionSec = progresses.firstOrNull { it.fileIdx == fileIdx }
                    ?.takeUnless { it.completed }?.positionSeconds ?: 0.0
                return@LaunchedEffect
            } catch (e: retrofit2.HttpException) {
                if (e.code() == 401 || e.code() == 404) {
                    error = "Probe failed (HTTP ${e.code()})"
                    return@LaunchedEffect
                }
                attempts++
                delay(2_000)
            } catch (_: Exception) {
                attempts++
                delay(2_000)
            }
        }
        error = "Timed out waiting for the file to download enough to probe"
    }

    // Live torrent state — drives the "Downloading …" step in the loading
    // overlay so the user sees real bytes / speed while the source is
    // still being pulled. Polls every 2s for as long as the screen lives.
    LaunchedEffect(infohash) {
        while (true) {
            val url = container.sessionStore.serverUrl.first() ?: run {
                delay(2_000); continue
            }
            runCatching { container.apiFor(url).getTorrent(infohash) }
                .onSuccess { torrent = it }
            delay(2_000)
        }
    }

    // Playback prep status. Polls every second until the HLS master
    // playlist is on disk. The endpoint surfaces both the upstream
    // torrent download and the in-flight ffmpeg+shaka pipeline so the
    // loading overlay can show a meaningful step (and the remux %).
    LaunchedEffect(probe, serverUrl) {
        val baseUrl = serverUrl ?: return@LaunchedEffect
        probe ?: return@LaunchedEffect
        playStatus = null
        val api = container.apiFor(baseUrl)
        while (true) {
            val s = runCatching { api.playStatus(infohash, fileIdx) }.getOrNull()
            if (s != null) {
                playStatus = s
                // Stop polling once the cache is ready OR a sticky failure
                // is surfaced — both are terminal until the user retries.
                if (s.ready || s.error != null) break
            }
            delay(1_000)
        }
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black),
    ) {
        val ready = playStatus?.ready == true && probe != null && serverUrl != null
        if (ready) {
            ReadyPlayer(
                container = container,
                serverUrl = serverUrl!!,
                infohash = infohash,
                fileIdx = fileIdx,
                probe = probe!!,
                torrent = torrent,
                startPositionSec = resumePositionSec,
                onPositionUpdate = { resumePositionSec = it },
                onPlayerError = { error = it },
            )
        } else {
            LoadingOverlay(
                error = error,
                status = playStatus,
                probeReady = probe != null,
                torrent = torrent,
                onRetry = {
                    probe = null
                    playStatus = null
                    error = null
                    probeVersion++
                },
                onBack = onBack,
            )
        }
    }
}

@Composable
private fun ReadyPlayer(
    container: AppContainer,
    serverUrl: String,
    infohash: String,
    fileIdx: Int,
    probe: MediaProbe,
    torrent: TorrentView?,
    startPositionSec: Double,
    onPositionUpdate: (Double) -> Unit,
    onPlayerError: (String) -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()

    // "Prepare next?" state. Fetched once at mount; the tick
    // (below) flips `nextEpModalOpen` when playback crosses 95 %.
    var nextEpisode by remember(infohash, fileIdx) {
        mutableStateOf<EpisodePoint?>(null)
    }
    var currentEpisode by remember(infohash, fileIdx) {
        mutableStateOf<EpisodePoint?>(null)
    }
    var nextEpModalOpen by remember(infohash, fileIdx) {
        mutableStateOf(false)
    }
    var nextEpDismissed by remember(infohash, fileIdx) {
        mutableStateOf(false)
    }
    var nextEpGrabbing by remember { mutableStateOf(false) }
    LaunchedEffect(infohash, fileIdx) {
        val ctx = runCatching {
            container.apiFor(serverUrl).episodeContext(infohash, fileIdx)
        }.getOrNull()
        currentEpisode = ctx?.current
        nextEpisode = ctx?.next?.takeIf {
            ctx.followed && it.status == "available"
        }
    }

    val playUrl = remember(serverUrl, infohash, fileIdx) {
        // HLS-CMAF master playlist — same URL the web client uses.
        // The bare `/play` path was the old single-file fMP4 endpoint;
        // the current backend serves the manifest tree under
        // `/play/{asset}` and the entry point is `master.m3u8`.
        val base = if (serverUrl.endsWith("/")) serverUrl else "$serverUrl/"
        "${base}api/torrents/$infohash/files/$fileIdx/play/master.m3u8"
    }

    val subtitles = remember(probe, serverUrl, infohash, fileIdx) {
        probe.subtitle.filter { it.textBased }.map { sub ->
            val base = if (serverUrl.endsWith("/")) serverUrl else "$serverUrl/"
            val url = "${base}api/torrents/$infohash/files/$fileIdx/sub/${sub.index}/track.vtt"
            // SDH detection from the source title — ffmpeg / Matroska
            // muxers tag closed-caption tracks with strings like "SDH",
            // "CC", "captions" or "hearing impaired". When matched we
            // set the role flags Media3's DefaultTrackNameProvider
            // recognises so the track renders as "Language · Closed
            // captions" in the native menu.
            val sdh = sub.title?.lowercase()?.let { t ->
                "sdh" in t || "captions" in t || "hearing" in t || t.trim() == "cc"
            } == true
            var selection = 0
            if (sub.default) selection = selection or C.SELECTION_FLAG_DEFAULT
            if (sub.forced) selection = selection or C.SELECTION_FLAG_FORCED
            var roles = 0
            if (sdh) roles = roles or C.ROLE_FLAG_CAPTION or C.ROLE_FLAG_DESCRIBES_MUSIC_AND_SOUND
            MediaItem.SubtitleConfiguration.Builder(url.toUri())
                .setMimeType(MimeTypes.TEXT_VTT)
                .setLanguage(sub.language ?: "und")
                .setLabel(sub.title ?: sub.language?.uppercase() ?: "Sub ${sub.index + 1}")
                .setSelectionFlags(selection)
                .setRoleFlags(roles)
                .build()
        }
    }

    val title by remember(torrent?.name, currentEpisode) {
        mutableStateOf(buildPlaybackTitle(torrent?.name, currentEpisode))
    }

    val player = remember(playUrl) {
        buildPlayer(context, container.okHttpClient).apply {
            setMediaItem(
                buildMediaItem(playUrl, subtitles, title),
                (startPositionSec * 1000).toLong().coerceAtLeast(0),
            )
            prepare()
            playWhenReady = true
        }
    }


    // Honour the file's `default` flag for audio/subtitles on first mount.
    // The native settings gear (`exo_settings`) handles runtime switching;
    // this just keeps the initial pick from defaulting to whatever Media3
    // guesses from the system locale.
    LaunchedEffect(player, probe) {
        val initialAudio = probe.audio.firstOrNull { it.default }?.language
            ?: probe.audio.firstOrNull()?.language
        val initialSub = probe.subtitle.firstOrNull { it.default }?.language
        val params = player.trackSelectionParameters.buildUpon()
        if (initialAudio != null) params.setPreferredAudioLanguage(initialAudio)
        if (initialSub != null) {
            params.setPreferredTextLanguage(initialSub)
            params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
        } else {
            params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
        }
        player.trackSelectionParameters = params.build()
    }

    // Surface ExoPlayer errors back to the parent so the LoadingOverlay
    // can show them. Without this any decoder / network failure leaves
    // the player paused at 00:00 with no indication of what went wrong
    // (this is exactly how the silent `/play` 404 hid for a session).
    DisposableEffect(player) {
        val listener = object : androidx.media3.common.Player.Listener {
            override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
                onPlayerError(error.errorCodeName + ": " + (error.message ?: "playback failed"))
            }
        }
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
    }

    DisposableEffect(player) {
        val handler = android.os.Handler(android.os.Looper.getMainLooper())
        var lastSavedMs: Long = (startPositionSec * 1000).toLong()
        var durationMs: Long = -1
        var prompted = false
        val tick = object : Runnable {
            override fun run() {
                if (player.duration > 0) durationMs = player.duration
                val pos = player.currentPosition
                if (pos > 0) {
                    onPositionUpdate(pos / 1000.0)
                    if (pos - lastSavedMs >= 7_000) {
                        lastSavedMs = pos
                        val completed = durationMs > 0 && pos >= durationMs - 30_000
                        scope.launch {
                            runCatching {
                                container.apiFor(serverUrl).saveProgress(
                                    infohash = infohash,
                                    idx = fileIdx,
                                    body = ProgressUpdate(
                                        positionSeconds = pos / 1000.0,
                                        durationSeconds = if (durationMs > 0) durationMs / 1000.0 else null,
                                        completed = completed,
                                    ),
                                )
                            }
                        }
                    }
                    // "Prepare next?" trigger — at 95 % of
                    // duration once we know it. Single-shot per mount.
                    if (
                        !prompted &&
                        !nextEpDismissed &&
                        nextEpisode != null &&
                        durationMs > 0 &&
                        pos.toFloat() / durationMs.toFloat() >= 0.95f
                    ) {
                        prompted = true
                        nextEpModalOpen = true
                    }
                }
                handler.postDelayed(this, 1_000)
            }
        }
        handler.postDelayed(tick, 1_000)

        onDispose {
            handler.removeCallbacksAndMessages(null)
            val pos = player.currentPosition
            val dur = player.duration.takeIf { it > 0 }
            if (pos > 0) onPositionUpdate(pos / 1000.0)
            scope.launch {
                runCatching {
                    container.apiFor(serverUrl).saveProgress(
                        infohash = infohash,
                        idx = fileIdx,
                        body = ProgressUpdate(
                            positionSeconds = pos / 1000.0,
                            durationSeconds = dur?.div(1000.0),
                            completed = dur != null && pos >= dur - 30_000,
                        ),
                    )
                }
            }
            player.release()
        }
    }

    // The title TextView lives inside our overridden controller layout
    // (`@+id/iris_title`). Held here so we can keep it in sync with the
    // S/E suffix once the episode context resolves.
    var titleView by remember { mutableStateOf<TextView?>(null) }
    LaunchedEffect(titleView, title) {
        titleView?.text = title
    }

    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { ctx ->
            PlayerView(ctx).apply {
                this.player = player
                useController = true
                setShowSubtitleButton(true)
                setShowFastForwardButton(true)
                setShowRewindButton(true)
                controllerAutoShow = true
                layoutParams = android.widget.FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT)
                // Hold the screen-on flag for as long as the PlayerView is
                // attached. Without this, Android TV blanks the panel after
                // its idle timeout (typically 2-5 min) since the remote
                // sees no key events during continuous playback. The flag
                // is dropped automatically when the view is detached.
                keepScreenOn = true
                titleView = findViewById(R.id.iris_title)
                installIrisTrackNameProvider(this)
            }
        },
        update = { it.player = player },
    )

    val nextEp = nextEpisode
    if (nextEpModalOpen && nextEp != null) {
        androidx.compose.material3.AlertDialog(
            onDismissRequest = {
                nextEpModalOpen = false
                nextEpDismissed = true
            },
            title = {
                androidx.compose.material3.Text("Next episode available")
            },
            text = {
                androidx.compose.material3.Text(
                    "S%02dE%02d is ready to grab. Prepare it for the next session?"
                        .format(nextEp.season, nextEp.episode),
                )
            },
            confirmButton = {
                androidx.compose.material3.TextButton(
                    enabled = !nextEpGrabbing && nextEp.followId != null,
                    onClick = {
                        val fid = nextEp.followId ?: return@TextButton
                        nextEpGrabbing = true
                        scope.launch {
                            runCatching {
                                container.apiFor(serverUrl).grabEpisode(
                                    id = fid,
                                    season = nextEp.season,
                                    episode = nextEp.episode,
                                )
                            }
                            nextEpGrabbing = false
                            nextEpModalOpen = false
                        }
                    },
                ) {
                    androidx.compose.material3.Text(
                        if (nextEpGrabbing) "Preparing…" else "Prepare",
                    )
                }
            },
            dismissButton = {
                androidx.compose.material3.TextButton(
                    onClick = {
                        nextEpModalOpen = false
                        nextEpDismissed = true
                    },
                ) {
                    androidx.compose.material3.Text("Later")
                }
            },
        )
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun LoadingOverlay(
    error: String?,
    status: PlayStatus?,
    probeReady: Boolean,
    torrent: TorrentView?,
    onRetry: () -> Unit,
    onBack: () -> Unit,
) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(
            verticalArrangement = Arrangement.spacedBy(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.padding(32.dp),
        ) {
            if (error != null) {
                Text(
                    text = error,
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.error,
                )
                Row(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    modifier = Modifier.padding(top = 12.dp),
                ) {
                    Button(
                        onClick = onRetry,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 10.dp),
                    ) {
                        Text("Retry")
                    }
                    Button(
                        onClick = onBack,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 10.dp),
                    ) {
                        Text("Back")
                    }
                }
                return@Column
            }
            val (label, sub, pct) = stepFor(status, probeReady, torrent)
            Text(label, style = MaterialTheme.typography.titleMedium)
            sub?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (pct != null) {
                LinearProgressIndicator(
                    progress = { pct },
                    modifier = Modifier.width(360.dp),
                )
            } else {
                LinearProgressIndicator(modifier = Modifier.width(240.dp))
            }
        }
    }
}

private data class Step(val label: String, val sub: String?, val pct: Float?)

private fun stepFor(status: PlayStatus?, probeReady: Boolean, torrent: TorrentView?): Step {
    if (!probeReady) {
        // The file isn't on disk yet. If we know the torrent's overall
        // download state, surface that — the user wants "I have 320 MB
        // out of 4 GB, going at 8 MB/s" not "ffprobe scanning streams".
        if (torrent != null && torrent.progressPct < 99.9f) {
            val pct = torrent.progressPct / 100f
            val sub = buildString {
                append(formatBytesShort(torrent.progressBytes))
                append(" / ")
                append(formatBytesShort(torrent.totalSizeBytes))
                if (torrent.downloadSpeedBps > 0) {
                    append(" · ")
                    append(formatSpeedShort(torrent.downloadSpeedBps))
                }
                if (torrent.peers > 0) {
                    append(" · ${torrent.peers} peers")
                }
            }
            val label = if (torrent.state.equals("paused", ignoreCase = true)) {
                "Download paused"
            } else if (torrent.error != null) {
                "Torrent error"
            } else if (torrent.downloadSpeedBps > 0 || torrent.peers > 0) {
                "Downloading…"
            } else {
                "Connecting to peers…"
            }
            return Step(label, sub, pct.coerceIn(0f, 0.99f))
        }
        return Step("Reading media metadata…", "ffprobe scanning streams.", null)
    }
    val s = status ?: return Step("Starting playback prep…", null, null)
    if (s.error != null) return Step("Playback prep failed", s.error, null)
    return when (s.reason) {
        "downloading" -> {
            val pct = s.progress?.coerceIn(0f, 0.99f)
                ?: torrent?.let { (it.progressPct / 100f).coerceIn(0f, 0.99f) }
            val sub = torrent?.let {
                buildString {
                    append(formatBytesShort(it.progressBytes))
                    append(" / ")
                    append(formatBytesShort(it.totalSizeBytes))
                    if (it.downloadSpeedBps > 0) {
                        append(" · ")
                        append(formatSpeedShort(it.downloadSpeedBps))
                    }
                    if (it.peers > 0) append(" · ${it.peers} peers")
                }
            }
            Step("Downloading…", sub, pct)
        }
        "remuxing" -> {
            // ffmpeg's encoded-so-far position lands here once the
            // server has parsed its first `out_time_us` block (~1s
            // after spawn). Until then `s.progress` is null and we
            // show an indeterminate bar via the null `pct`.
            val pct = s.progress?.coerceIn(0f, 0.99f)
            val label = if (pct != null) {
                "Remuxing to fragmented MP4 · ${(pct * 100f).toInt()}%"
            } else {
                "Remuxing to fragmented MP4…"
            }
            Step(
                label,
                "Producing the playable cache (video copied as-is, audio re-encoded to AAC where needed).",
                pct,
            )
        }
        else -> Step("Preparing playback…", "Almost there.", null)
    }
}

private fun buildPlaybackTitle(rawName: String?, episode: EpisodePoint?): String {
    val pretty = rawName
        ?.substringBeforeLast('.', rawName)
        ?.replace('.', ' ')
        ?.replace('_', ' ')
        ?.trim()
        ?.takeIf { it.isNotBlank() }
        ?: "Now playing"
    return if (episode != null) {
        "$pretty · S%02dE%02d".format(episode.season, episode.episode)
    } else pretty
}

private fun formatBytesShort(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format(java.util.Locale.ROOT, "%.0f MB", mb)
    return "$b B"
}

private fun formatSpeedShort(bps: Long): String {
    val mbs = bps / 1_000_000.0
    if (mbs >= 1.0) return String.format(java.util.Locale.ROOT, "%.1f MB/s", mbs)
    val kbs = bps / 1_000.0
    return String.format(java.util.Locale.ROOT, "%.0f KB/s", kbs)
}
