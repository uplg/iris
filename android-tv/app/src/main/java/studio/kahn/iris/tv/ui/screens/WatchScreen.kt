package studio.kahn.iris.tv.ui.screens

import android.net.Uri
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.window.Dialog
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.PlayerView
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Surface
import androidx.tv.material3.Button
import androidx.tv.material3.ButtonDefaults
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Text
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.AudioStream
import studio.kahn.iris.tv.data.HlsStatus
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.MediaProbe
import studio.kahn.iris.tv.data.ProgressUpdate
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.buildMediaItem
import studio.kahn.iris.tv.data.buildPlayer

/**
 * Full-screen Media3 PlayerView. Pre-mount we poll `/hls/.../status` so the
 * user sees actual ffmpeg progression instead of a silent black screen, and
 * we only construct the player when ENDLIST is on disk — same trick as
 * the web client.
 *
 * D-pad maps to PlayerView's built-in TV controls (play/pause, seek,
 * subtitles via the CC button). Audio-track switching is a custom dialog
 * because each Iris HLS playlist is single-audio (one ffmpeg job per
 * audioIdx) — picking a track means loading a different master URL, and
 * waiting for that pipeline's ENDLIST.
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
    var hlsStatus by remember { mutableStateOf<HlsStatus?>(null) }
    var torrent by remember { mutableStateOf<TorrentView?>(null) }
    var resumePositionSec by remember { mutableStateOf(0.0) }
    var error by remember { mutableStateOf<String?>(null) }
    var selectedAudioIdx by remember { mutableStateOf<Int?>(null) }
    var showAudioPicker by remember { mutableStateOf(false) }
    var probeVersion by remember { mutableStateOf(0) }
    // Tied to the Media3 PlayerView controller via setControllerVisibilityListener.
    // When the controller is hidden during playback, our custom audio chip
    // hides too — full-screen video without a "FRE" tag in the corner.
    var controllerVisible by remember { mutableStateOf(true) }

    // Probe with retry. When the user clicks Play right after ingest, the
    // file isn't on disk yet — librqbit needs a few seconds to fetch the
    // first sequential chunks. The server returns 400 "file not yet on
    // disk: …" in that window. We retry every 2s for up to ~2 min so the
    // user just sees the LoadingOverlay tick down instead of bouncing
    // straight to an error screen. 404 / 401 / unknown errors bail
    // immediately — those are real problems.
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
                val p = api.probe(infohash, fileIdx)
                probe = p
                if (selectedAudioIdx == null) {
                    selectedAudioIdx = (p.audio.firstOrNull { it.default } ?: p.audio.firstOrNull())?.index ?: 0
                }
                val progresses = runCatching { api.torrentProgress(infohash) }.getOrDefault(emptyList())
                resumePositionSec = progresses.firstOrNull { it.fileIdx == fileIdx }
                    ?.takeUnless { it.completed }?.positionSeconds ?: 0.0
                return@LaunchedEffect
            } catch (e: retrofit2.HttpException) {
                if (e.code() == 401 || e.code() == 404) {
                    error = "Probe failed (HTTP ${e.code()})"
                    return@LaunchedEffect
                }
                // 400 == "file not yet on disk" most of the time, possibly
                // a 5xx burp — keep waiting.
                attempts++
                delay(2_000)
            } catch (e: Exception) {
                // Connection blip, parse error — try again briefly.
                attempts++
                delay(2_000)
            }
        }
        error = "Timed out waiting for the file to download enough to probe"
    }

    // Live torrent state — drives the "Downloading …" step in the loading
    // overlay so the user sees real bytes / speed while the ingest is still
    // pulling sequential chunks. Polls every 2s for as long as the screen
    // is mounted.
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

    // HLS status polling. The pipeline now produces a single master playlist
    // with all audio renditions baked in (#EXT-X-MEDIA), so progress no
    // longer depends on the audio pick — there's exactly one ffmpeg job per
    // file. Switching audio is a player-side toggle.
    LaunchedEffect(probe, serverUrl) {
        val baseUrl = serverUrl ?: return@LaunchedEffect
        probe ?: return@LaunchedEffect
        hlsStatus = null
        val api = container.apiFor(baseUrl)
        while (true) {
            val s = runCatching { api.hlsStatus(infohash, fileIdx) }.getOrNull()
            if (s != null) {
                hlsStatus = s
                if (s.endlistPresent) break
            }
            delay(1_000)
        }
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black),
    ) {
        // Start the player as soon as we have ~3 segments on disk. ffmpeg
        // keeps writing in the background and Media3 / hls.js auto-discover
        // the new segments via the EVENT-type playlist. Waiting for
        // ENDLIST blocked the player for minutes on slow boxes.
        val ready = (hlsStatus?.endlistPresent == true
            || (hlsStatus?.segmentsProduced ?: 0) >= 3)
            && probe != null
            && serverUrl != null
        if (ready) {
            ReadyPlayer(
                container = container,
                serverUrl = serverUrl!!,
                infohash = infohash,
                fileIdx = fileIdx,
                probe = probe!!,
                preferredAudioIdx = selectedAudioIdx,
                startPositionSec = resumePositionSec,
                onPositionUpdate = { resumePositionSec = it },
                onAudioPicked = { selectedAudioIdx = it },
                onControllerVisibilityChanged = { controllerVisible = it },
            )
            // Audio switcher overlay — only shown if there's more than one
            // audio track to pick from AND the player controller is up
            // (the user pressed something). During real playback the chip
            // disappears with the rest of the controls so video stays
            // full-screen.
            if ((probe?.audio?.size ?: 0) > 1 && controllerVisible) {
                Surface(
                    onClick = { showAudioPicker = true },
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .padding(24.dp),
                    shape = RoundedCornerShape(20.dp),
                    color = Color.Black.copy(alpha = 0.55f),
                    contentColor = Color.White,
                ) {
                    Text(
                        "Audio · " + currentAudioLabel(probe!!.audio, selectedAudioIdx ?: -1),
                        style = MaterialTheme.typography.labelMedium,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                    )
                }
            }
        } else {
            LoadingOverlay(
                error = error,
                status = hlsStatus,
                probeReady = probe != null,
                torrent = torrent,
                onRetry = {
                    probe = null
                    hlsStatus = null
                    error = null
                    probeVersion++
                },
                onBack = onBack,
            )
        }

        if (showAudioPicker && probe != null) {
            AudioPickerDialog(
                tracks = probe!!.audio,
                selected = selectedAudioIdx ?: -1,
                onSelect = { newIdx ->
                    showAudioPicker = false
                    selectedAudioIdx = newIdx
                },
                onDismiss = { showAudioPicker = false },
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
    preferredAudioIdx: Int?,
    startPositionSec: Double,
    onPositionUpdate: (Double) -> Unit,
    onAudioPicked: (Int) -> Unit,
    onControllerVisibilityChanged: (Boolean) -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()

    // Single master URL for the whole file — switching audio is a track
    // selection parameter, not a URL change.
    val masterUrl = remember(serverUrl, infohash, fileIdx) {
        val base = if (serverUrl.endsWith("/")) serverUrl else "$serverUrl/"
        "${base}api/torrents/$infohash/files/$fileIdx/hls/master.m3u8"
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

    val player = remember(masterUrl) {
        buildPlayer(context, container.okHttpClient).apply {
            setMediaItem(
                buildMediaItem(masterUrl, subtitles, startPositionSec),
                (startPositionSec * 1000).toLong(),
            )
            prepare()
            playWhenReady = true
        }
    }

    // Track selection bookkeeping. ExoPlayer announces the available audio
    // groups asynchronously after the HLS manifest is parsed, so we can't
    // just apply the override once at mount — we'd usually get there with
    // an empty `currentTracks`. Instead we listen for `onTracksChanged`
    // and re-apply whenever either (a) tracks become available, or (b) the
    // user picks a different audio idx.
    var tracksTick by remember { mutableStateOf(0) }
    DisposableEffect(player) {
        val listener = object : androidx.media3.common.Player.Listener {
            override fun onTracksChanged(tracks: androidx.media3.common.Tracks) {
                tracksTick++
            }
        }
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
    }

    // Apply the preferred audio choice as a hard `TrackSelectionOverride`.
    // We don't use `setPreferredAudioLanguage` because (a) two tracks could
    // share the same language tag, (b) ExoPlayer's language matching is
    // fuzzy and sometimes "fra" != "fr" silently. The override targets the
    // exact group at the right ordinal — the order in `probe.audio` mirrors
    // the order ffmpeg emitted in `var_stream_map`, which is the same
    // order the HLS master playlist exposes.
    LaunchedEffect(player, preferredAudioIdx, tracksTick) {
        val idx = preferredAudioIdx ?: return@LaunchedEffect
        val ordinal = probe.audio.indexOfFirst { it.index == idx }
        if (ordinal < 0) return@LaunchedEffect
        val audioGroups = player.currentTracks.groups.filter {
            it.type == androidx.media3.common.C.TRACK_TYPE_AUDIO
        }
        val target = audioGroups.getOrNull(ordinal) ?: return@LaunchedEffect
        player.trackSelectionParameters = player.trackSelectionParameters
            .buildUpon()
            .setOverrideForType(
                androidx.media3.common.TrackSelectionOverride(target.mediaTrackGroup, 0)
            )
            .build()
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
                                        audioTrackIdx = preferredAudioIdx,
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
                            audioTrackIdx = preferredAudioIdx,
                            completed = dur != null && pos >= dur - 30_000,
                        ),
                    )
                }
            }
            player.release()
        }
    }

    // Silences a "lint: unused parameter" — the callback is wired so future
    // versions can route audio picks to track selection by ID instead of by
    // language. For now [LaunchedEffect] above does the work.
    @Suppress("unused") val _onAudioPicked = onAudioPicked

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
                // Mirror the controller's show/hide state into Compose so
                // our custom audio overlay can fade in & out together with
                // the rest of the player chrome.
                setControllerVisibilityListener(
                    PlayerView.ControllerVisibilityListener { visibility ->
                        onControllerVisibilityChanged(visibility == android.view.View.VISIBLE)
                    }
                )
            }
        },
        update = { it.player = player },
    )
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun LoadingOverlay(
    error: String?,
    status: HlsStatus?,
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

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun AudioPickerDialog(
    tracks: List<AudioStream>,
    selected: Int,
    onSelect: (Int) -> Unit,
    onDismiss: () -> Unit,
) {
    Dialog(onDismissRequest = onDismiss) {
        Surface(
            shape = RoundedCornerShape(16.dp),
            color = MaterialTheme.colorScheme.surface,
            contentColor = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.width(460.dp),
        ) {
            Column(
                Modifier.padding(24.dp).fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    "Audio track",
                    style = MaterialTheme.typography.titleLarge,
                    modifier = Modifier.padding(bottom = 8.dp),
                )
                tracks.forEach { track ->
                    val isSelected = track.index == selected
                    val parts = listOfNotNull(
                        track.language?.uppercase(),
                        track.codec.uppercase(),
                        "${track.channels} ch",
                        track.title?.takeIf { it.isNotBlank() },
                    )
                    Surface(
                        onClick = { onSelect(track.index) },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(8.dp),
                        color = if (isSelected) {
                            MaterialTheme.colorScheme.primaryContainer
                        } else {
                            MaterialTheme.colorScheme.surfaceVariant
                        },
                        contentColor = if (isSelected) {
                            MaterialTheme.colorScheme.onPrimaryContainer
                        } else {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        },
                    ) {
                        Text(
                            (if (isSelected) "● " else "○ ") + parts.joinToString(" · "),
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
        }
    }
}

private fun currentAudioLabel(tracks: List<AudioStream>, idx: Int): String {
    val t = tracks.firstOrNull { it.index == idx } ?: return "Default"
    return t.language?.uppercase() ?: t.codec.uppercase()
}

private data class Step(val label: String, val sub: String?, val pct: Float?)

private fun stepFor(status: HlsStatus?, probeReady: Boolean, torrent: TorrentView?): Step {
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
    val s = status ?: return Step("Starting transcoder…", null, null)
    // Pre-roll: we only show this until ~3 segments are on disk, then
    // playback starts. This branch covers the very-first-seconds window
    // (segments_produced 0..2) where ffmpeg is still spinning up.
    val seg = s.segmentsProduced
    return Step(
        "Buffering first frames…",
        "$seg segment${if (seg > 1) "s" else ""} ready",
        if (seg > 0) (seg.toFloat() / 3f).coerceIn(0f, 1f) else null,
    )
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
