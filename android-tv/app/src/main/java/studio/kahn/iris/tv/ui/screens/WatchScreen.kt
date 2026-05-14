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
    var manifest by remember { mutableStateOf<studio.kahn.iris.tv.data.Manifest?>(null) }
    var torrent by remember { mutableStateOf<TorrentView?>(null) }
    var resumePositionSec by remember { mutableStateOf(0.0) }
    // Saved audio + subtitle stream indices from the previous session.
    // `null` means "no preference yet, fall back to the source file's
    // `default` flag" (initial-mount behaviour for first-time watches).
    var savedAudioIdx by remember { mutableStateOf<Int?>(null) }
    var savedSubIdx by remember { mutableStateOf<Int?>(null) }
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
                // Also fetch the single-file progress to recover the
                // user's previous audio + subtitle picks. The bulk
                // endpoint above only carries position; this one has
                // the per-file `audio_track_idx` + `subtitle_track_idx`
                // the player will hand back to `trackSelectionParameters`
                // at mount. Safe to ignore failures (e.g. the user
                // has never watched this file before → `null`).
                val saved = runCatching { api.getProgress(infohash, fileIdx) }.getOrNull()
                savedAudioIdx = saved?.audioTrackIdx
                savedSubIdx = saved?.subtitleTrackIdx
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

    // Phase 0 of the capability-negotiated pipeline: fetch the manifest
    // once the probe succeeds. Phase 0 keeps using /play/master.m3u8 for
    // actual playback; the manifest call here exists to validate the wire
    // contract end-to-end and seed Phase 3's switch to direct-blob play.
    LaunchedEffect(probe, serverUrl) {
        val baseUrl = serverUrl ?: return@LaunchedEffect
        probe ?: return@LaunchedEffect
        if (manifest != null) return@LaunchedEffect
        runCatching { container.apiFor(baseUrl).manifest(infohash, fileIdx) }
            .onSuccess { m ->
                manifest = m
                android.util.Log.i(
                    "iris-core",
                    "manifest: container=${m.container} v=${m.video.map { it.codecString ?: it.codec }} " +
                        "a=${m.audio.size} subs=${m.subtitles.size} hdr=${m.video.firstOrNull()?.hdr}",
                )
            }
            .onFailure {
                android.util.Log.w("iris-core", "manifest fetch failed: ${it.message}")
            }
    }

    // No more `/play/status` polling — that endpoint signals when the
    // server-side HLS-CMAF cache (ffmpeg+shaka) is ready. We now feed
    // the raw `/stream` bytes straight into Media3, which demuxes and
    // decodes everything in-process. The probe call above is the only
    // gate we wait on (it confirms the file is at least partially on
    // disk and ffprobe scanned its streams).

    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black),
    ) {
        val ready = probe != null && serverUrl != null
        if (ready) {
            ReadyPlayer(
                container = container,
                serverUrl = serverUrl!!,
                infohash = infohash,
                fileIdx = fileIdx,
                probe = probe!!,
                torrent = torrent,
                startPositionSec = resumePositionSec,
                initialAudioIdx = savedAudioIdx,
                initialSubIdx = savedSubIdx,
                onPositionUpdate = { resumePositionSec = it },
                onPlayerError = { error = it },
            )
        } else {
            LoadingOverlay(
                error = error,
                probeReady = probe != null,
                torrent = torrent,
                onRetry = {
                    probe = null
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
    initialAudioIdx: Int?,
    initialSubIdx: Int?,
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
        // Raw source bytes — Media3 demuxes the container in-process
        // and surfaces every audio + subtitle track (incl. PGS bitmap
        // subs from Blu-rays) via the native track selector. No
        // server-side remux needed.
        val base = if (serverUrl.endsWith("/")) serverUrl else "$serverUrl/"
        "${base}api/torrents/$infohash/files/$fileIdx/stream"
    }

    // No more external subtitle injection — the source MKV / MP4
    // already contains every subtitle track and Media3 has parsers
    // for SRT, ASS/SSA and PGS bitmap. Defaults are honored below
    // via `trackSelectionParameters` once the player surfaces the
    // available tracks.

    val title by remember(torrent?.name, currentEpisode) {
        mutableStateOf(buildPlaybackTitle(torrent?.name, currentEpisode))
    }

    val player = remember(playUrl) {
        buildPlayer(context, container.okHttpClient).apply {
            setMediaItem(
                buildMediaItem(playUrl, title),
                (startPositionSec * 1000).toLong().coerceAtLeast(0),
            )
            prepare()
            playWhenReady = true
        }
    }


    // Pick initial audio + subtitle preferences. Priority order:
    //   1. The user's saved pick from the previous session
    //      (via `initialAudioIdx` / `initialSubIdx`, resolved by
    //      stream index → language through the probe).
    //   2. The source file's `default` flag.
    //   3. Whatever Media3 guesses from the system locale.
    // The native settings gear (`exo_settings`) handles further
    // runtime switching, which we observe further below to keep the
    // saved prefs fresh.
    LaunchedEffect(player, probe, initialAudioIdx, initialSubIdx) {
        val audioFromSaved = initialAudioIdx?.let { idx ->
            probe.audio.firstOrNull { it.index == idx }?.language
        }
        val initialAudio = audioFromSaved
            ?: probe.audio.firstOrNull { it.default }?.language
            ?: probe.audio.firstOrNull()?.language
        // For subs: `initialSubIdx == null` means "user has no saved
        // preference yet" → fall back to the source `default` flag.
        // `initialSubIdx == -1` is the explicit "no subs" sentinel
        // (user actively disabled subtitles last session); honour it.
        val subFromSaved = when (initialSubIdx) {
            null -> probe.subtitle.firstOrNull { it.default }?.language
            -1 -> null
            else -> probe.subtitle.firstOrNull { it.index == initialSubIdx }?.language
        }
        val subDisabled = initialSubIdx == -1
        val params = player.trackSelectionParameters.buildUpon()
        if (initialAudio != null) params.setPreferredAudioLanguage(initialAudio)
        if (subFromSaved != null) {
            params.setPreferredTextLanguage(subFromSaved)
            params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
        } else if (subDisabled) {
            params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
        } else {
            params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
        }
        player.trackSelectionParameters = params.build()
    }

    // Track the user's current audio + subtitle picks so subsequent
    // `saveProgress` calls can persist them. We update via
    // `onTracksChanged` rather than per-tick polling: tracks only
    // change when the user opens the settings menu and clicks, so
    // event-driven is cheap and accurate.
    val currentAudioIdxRef = remember { java.util.concurrent.atomic.AtomicReference<Int?>(initialAudioIdx) }
    val currentSubIdxRef = remember { java.util.concurrent.atomic.AtomicReference<Int?>(initialSubIdx) }

    // ExoPlayer errors. Transient codes (network blip, mid-stream
    // decoder hiccup) trigger an auto-retry up to MAX_RETRIES times
    // within a sliding RETRY_WINDOW_MS — successful playback resets
    // the counter. Terminal codes (unsupported container, decoder
    // init failure, …) surface immediately so the user sees a real
    // message instead of "ERROR_CODE_FOO_BAR".
    DisposableEffect(player) {
        val maxRetries = 3
        val retryWindowMs = 30_000L
        var retryCount = 0
        var firstRetryAt = 0L
        val listener = object : androidx.media3.common.Player.Listener {
            override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
                val (message, transient) = studio.kahn.iris.tv.data.humanizePlaybackError(error)
                android.util.Log.w(
                    "iris-core",
                    "playback error ${error.errorCodeName} transient=$transient: ${error.message}",
                )
                val now = android.os.SystemClock.uptimeMillis()
                if (firstRetryAt == 0L || now - firstRetryAt > retryWindowMs) {
                    firstRetryAt = now
                    retryCount = 0
                }
                if (transient && retryCount < maxRetries) {
                    retryCount++
                    android.util.Log.i(
                        "iris-core",
                        "auto-retry #$retryCount after transient error",
                    )
                    scope.launch {
                        delay(1_500L * retryCount)
                        runCatching { player.prepare() }
                    }
                    return
                }
                onPlayerError(message)
            }

            override fun onPlaybackStateChanged(state: Int) {
                if (state == androidx.media3.common.Player.STATE_READY) {
                    // Successful resume — reset the retry budget.
                    retryCount = 0
                    firstRetryAt = 0L
                }
            }

            override fun onTracksChanged(tracks: androidx.media3.common.Tracks) {
                // Pull the user's current language picks from the
                // selected groups, then resolve back to the probe's
                // stream index so `saveProgress` can write something
                // the next session understands.
                val pickedAudioLang = tracks.groups
                    .firstOrNull { it.type == C.TRACK_TYPE_AUDIO && it.isSelected }
                    ?.let { g ->
                        (0 until g.length).firstOrNull { g.isTrackSelected(it) }
                            ?.let { i -> g.getTrackFormat(i).language }
                    }
                val pickedSubGroup = tracks.groups
                    .firstOrNull { it.type == C.TRACK_TYPE_TEXT && it.isSelected }
                val pickedSubLang = pickedSubGroup?.let { g ->
                    (0 until g.length).firstOrNull { g.isTrackSelected(it) }
                        ?.let { i -> g.getTrackFormat(i).language }
                }
                currentAudioIdxRef.set(
                    probe.audio.firstOrNull { it.language == pickedAudioLang }?.index
                        ?: currentAudioIdxRef.get(),
                )
                currentSubIdxRef.set(
                    if (pickedSubGroup == null || pickedSubLang == null) -1
                    else probe.subtitle.firstOrNull { it.language == pickedSubLang }?.index
                        ?: currentSubIdxRef.get(),
                )
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
                        val audioIdx = currentAudioIdxRef.get()
                        val subIdx = currentSubIdxRef.get()
                        scope.launch {
                            runCatching {
                                container.apiFor(serverUrl).saveProgress(
                                    infohash = infohash,
                                    idx = fileIdx,
                                    body = ProgressUpdate(
                                        positionSeconds = pos / 1000.0,
                                        durationSeconds = if (durationMs > 0) durationMs / 1000.0 else null,
                                        audioTrackIdx = audioIdx,
                                        subtitleTrackIdx = subIdx,
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
            val audioIdx = currentAudioIdxRef.get()
            val subIdx = currentSubIdxRef.get()
            scope.launch {
                runCatching {
                    container.apiFor(serverUrl).saveProgress(
                        infohash = infohash,
                        idx = fileIdx,
                        body = ProgressUpdate(
                            positionSeconds = pos / 1000.0,
                            durationSeconds = dur?.div(1000.0),
                            audioTrackIdx = audioIdx,
                            subtitleTrackIdx = subIdx,
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
            val (label, sub, pct) = stepFor(probeReady, torrent)
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

private fun stepFor(probeReady: Boolean, torrent: TorrentView?): Step {
    // The TV now plays the raw `/stream` endpoint directly — Media3
    // demuxes the container in-process, so there's no server-side
    // remux to wait for. The only gate is "did ffprobe scan the
    // file's streams" (probeReady), with the torrent download
    // surfaced underneath so the user sees real bytes / speed while
    // librqbit is still pulling the first chunks.
    if (probeReady) {
        return Step("Starting playback…", null, null)
    }
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
