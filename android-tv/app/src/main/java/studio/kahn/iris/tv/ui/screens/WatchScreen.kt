package studio.kahn.iris.tv.ui.screens

import android.net.Uri
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
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
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.MediaProbe
import studio.kahn.iris.tv.data.PlayStatus
import studio.kahn.iris.tv.data.ProgressUpdate
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.buildMediaItem
import studio.kahn.iris.tv.data.buildPlayer

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
@OptIn(ExperimentalTvMaterial3Api::class, UnstableApi::class)
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

@OptIn(UnstableApi::class)
@Composable
private fun ReadyPlayer(
    container: AppContainer,
    serverUrl: String,
    infohash: String,
    fileIdx: Int,
    probe: MediaProbe,
    startPositionSec: Double,
    onPositionUpdate: (Double) -> Unit,
    onPlayerError: (String) -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()

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
            MediaItem.SubtitleConfiguration.Builder(Uri.parse(url))
                .setMimeType(MimeTypes.TEXT_VTT)
                .setLanguage(sub.language ?: "und")
                .setLabel(sub.title ?: sub.language?.uppercase() ?: "Sub ${sub.index + 1}")
                .setSelectionFlags(if (sub.default) androidx.media3.common.C.SELECTION_FLAG_DEFAULT else 0)
                .build()
        }
    }

    val player = remember(playUrl) {
        buildPlayer(context, container.okHttpClient).apply {
            setMediaItem(
                buildMediaItem(playUrl, subtitles),
                (startPositionSec * 1000).toLong().coerceAtLeast(0),
            )
            prepare()
            playWhenReady = true
        }
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
            }
        },
        update = { it.player = player },
    )
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

private fun formatBytesShort(b: Long): String {
    val gb = b / 1_000_000_000.0
    if (gb >= 1.0) return String.format("%.1f GB", gb)
    val mb = b / 1_000_000.0
    if (mb >= 1.0) return String.format("%.0f MB", mb)
    return "$b B"
}

private fun formatSpeedShort(bps: Long): String {
    val mbs = bps / 1_000_000.0
    if (mbs >= 1.0) return String.format("%.1f MB/s", mbs)
    val kbs = bps / 1_000.0
    return String.format("%.0f KB/s", kbs)
}
