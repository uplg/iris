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
    var resumePositionSec by remember { mutableStateOf(0.0) }
    var error by remember { mutableStateOf<String?>(null) }
    var selectedAudioIdx by remember { mutableStateOf<Int?>(null) }
    var showAudioPicker by remember { mutableStateOf(false) }
    var probeVersion by remember { mutableStateOf(0) }

    LaunchedEffect(infohash, fileIdx, probeVersion) {
        error = null
        val url = container.sessionStore.serverUrl.first()
        if (url == null) {
            error = "Not signed in"
            return@LaunchedEffect
        }
        serverUrl = url
        val api: IrisApi = container.apiFor(url)
        try {
            val p = api.probe(infohash, fileIdx)
            probe = p
            if (selectedAudioIdx == null) {
                selectedAudioIdx = (p.audio.firstOrNull { it.default } ?: p.audio.firstOrNull())?.index ?: 0
            }
            val progresses = runCatching { api.torrentProgress(infohash) }.getOrDefault(emptyList())
            resumePositionSec = progresses.firstOrNull { it.fileIdx == fileIdx }
                ?.takeUnless { it.completed }?.positionSeconds ?: 0.0
        } catch (e: Exception) {
            error = e.message ?: "Failed to load media metadata"
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
        val ready = hlsStatus?.endlistPresent == true
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
            )
            // Audio switcher overlay — only shown if there's more than one
            // audio track to pick from. The dialog sets Media3 track
            // selection parameters; no player re-build, no segment re-fetch.
            if ((probe?.audio?.size ?: 0) > 1) {
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

    // Apply preferred audio whenever the user picks one (or saved progress
    // surfaces it). We translate `audioIdx` → ISO language preference, which
    // works because each rendition advertises its language in the master
    // playlist; ExoPlayer's track selector matches on it.
    LaunchedEffect(player, preferredAudioIdx) {
        val idx = preferredAudioIdx ?: return@LaunchedEffect
        val track = probe.audio.firstOrNull { it.index == idx } ?: return@LaunchedEffect
        val lang = track.language ?: return@LaunchedEffect
        player.trackSelectionParameters = player.trackSelectionParameters
            .buildUpon()
            .setPreferredAudioLanguage(lang)
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
            val (label, sub, pct) = stepFor(status, probeReady)
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

private fun stepFor(status: HlsStatus?, probeReady: Boolean): Step {
    if (!probeReady) return Step("Reading media metadata…", "ffprobe scanning streams.", null)
    val s = status ?: return Step("Starting transcoder…", null, null)
    if (s.endlistPresent) return Step("Loading first frames…", "Almost there.", null)
    val total = s.estimatedTotalSegments
    val seg = s.segmentsProduced
    val pct = if (total != null && total > 0) (seg.toFloat() / total).coerceIn(0f, 0.99f) else null
    val label = if (total != null) {
        "Pre-segmenting · $seg / ~$total segments"
    } else {
        "Pre-segmenting · $seg segments"
    }
    return Step(label, "ffmpeg writing the HLS playlist. Seek will be enabled when it finishes.", pct)
}
