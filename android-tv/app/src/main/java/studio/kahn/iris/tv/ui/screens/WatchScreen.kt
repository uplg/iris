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
import androidx.compose.runtime.mutableIntStateOf
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
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
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
    /** Swap the player to a different (infohash, file_idx) — used by
     *  the Netflix-style "Up next" pill to auto-advance between
     *  episodes. The caller pops the current WATCH entry and pushes
     *  the new one, so Back from the new episode skips the previous
     *  one entirely. */
    onNavigateToFile: (String, Int) -> Unit,
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
                // Fetch probe AND progress into locals first, then
                // commit them to state in one block at the end. The
                // previous version assigned `probe` first, which
                // immediately triggered a Compose recomposition and
                // could let `ReadyPlayer` mount with the stale
                // `resumePositionSec=0` (default) before the bulk
                // progress fetch wrote the real resume offset.
                // Effect: "Continue watching" entries always started
                // at 0 on TV. The `ExoPlayer` instance is built via
                // `remember(playUrl)` and only reads the start
                // position ONCE, so the late-arriving update never
                // got picked up.
                val freshProbe = api.probe(infohash, fileIdx)
                val progresses = runCatching { api.torrentProgress(infohash) }
                    .getOrDefault(emptyList())
                val resume = progresses.firstOrNull { it.fileIdx == fileIdx }
                    ?.takeUnless { it.completed }?.positionSeconds ?: 0.0
                // Per-file progress carries the audio + subtitle
                // picks; safe to ignore failures (first-time watch).
                val saved = runCatching { api.getProgress(infohash, fileIdx) }
                    .getOrNull()
                // Commit atomically. The order here matters: write
                // `resumePositionSec` BEFORE `probe` so the very
                // first recomposition that sees `probe != null`
                // already has the right start offset.
                resumePositionSec = resume
                savedAudioIdx = saved?.audioTrackIdx
                savedSubIdx = saved?.subtitleTrackIdx
                probe = freshProbe
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
                onBack = onBack,
                onNavigateToFile = onNavigateToFile,
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
    onBack: () -> Unit,
    onNavigateToFile: (String, Int) -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()

    // Episode navigation context — fetched once per mount. Drives
    // the small "Prev / Next" chips overlaid on the player. Both
    // sides may be null (movie / first episode / last episode), in
    // which case the corresponding chip simply doesn't render.
    var prevEpisode by remember(infohash, fileIdx) {
        mutableStateOf<EpisodePoint?>(null)
    }
    var nextEpisode by remember(infohash, fileIdx) {
        mutableStateOf<EpisodePoint?>(null)
    }
    var currentEpisode by remember(infohash, fileIdx) {
        mutableStateOf<EpisodePoint?>(null)
    }
    // Player.STATE_ENDED — used to surface the "End of series"
    // banner when there's no next episode to chain into. Reset to
    // false on STATE_READY so a buffer-underrun retry doesn't keep
    // the banner stuck on.
    var playerEnded by remember(infohash, fileIdx) {
        mutableStateOf(false)
    }
    // Crossed the 95 % mark at least once this mount. Drives the
    // bigger bottom-right "Up next" pill — Netflix-style nudge that
    // shows up around the credit roll, complementing the always-on
    // top-right chips. Deliberate action only (no countdown).
    var nearEnd by remember(infohash, fileIdx) {
        mutableStateOf(false)
    }
    // User explicitly closed the credit-roll pill. The top-right
    // chips stay; only the prominent bottom-right pill is silenced.
    var pillDismissed by remember(infohash, fileIdx) {
        mutableStateOf(false)
    }
    var grabbing by remember(infohash, fileIdx) { mutableStateOf(false) }
    LaunchedEffect(infohash, fileIdx) {
        val ctx = runCatching {
            container.apiFor(serverUrl).episodeContext(infohash, fileIdx)
        }.getOrNull()
        currentEpisode = ctx?.current
        nextEpisode = ctx?.next
        prevEpisode = ctx?.prev
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


    // Resolve the saved track stream indices to ORDINALS — the position
    // of each saved track within `probe.audio` / `probe.subtitle`. We
    // identify tracks by ordinal (not language) because:
    //   * Two audio tracks may share a language (or both have `null`
    //     language), so `setPreferredAudioLanguage` can't disambiguate.
    //   * MatroskaExtractor + Media3 surface each MKV track as its own
    //     `Tracks.Group` in source order, so the N-th audio group
    //     always corresponds to `probe.audio[N]` — a precise mapping.
    // For subs `-1` is the explicit "user disabled subtitles" sentinel;
    // `null` means "no saved preference, let the source default win".
    val savedAudioOrdinal = remember(probe, initialAudioIdx) {
        initialAudioIdx?.let { idx ->
            probe.audio.indexOfFirst { it.index == idx }.takeIf { it >= 0 }
        }
    }
    val savedSubOrdinal: Int? = remember(probe, initialSubIdx) {
        when (initialSubIdx) {
            null -> null
            -1 -> -1
            else -> probe.subtitle.indexOfFirst { it.index == initialSubIdx }.takeIf { it >= 0 }
        }
    }

    // Hint Media3 toward the saved language at load time so the very
    // first frames play with the right audio (avoids a brief "wrong
    // language" beat before the override applies). This is best-effort
    // — `setPreferredAudioLanguage` collapses multi-track ambiguity,
    // so the authoritative pin is the `TrackSelectionOverride` we apply
    // on the first `onTracksChanged` event further below.
    LaunchedEffect(player, probe, initialAudioIdx, initialSubIdx) {
        val savedAudioLang = savedAudioOrdinal?.let { probe.audio[it].language }
        val initialAudio = savedAudioLang
            ?: probe.audio.firstOrNull { it.default }?.language
            ?: probe.audio.firstOrNull()?.language
        val savedSubLang = savedSubOrdinal
            ?.takeIf { it >= 0 && it in probe.subtitle.indices }
            ?.let { probe.subtitle[it].language }
        val params = player.trackSelectionParameters.buildUpon()
        if (initialAudio != null) params.setPreferredAudioLanguage(initialAudio)
        if (savedSubLang != null) {
            params.setPreferredTextLanguage(savedSubLang)
            params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
        } else if (savedSubOrdinal == -1) {
            params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
        } else {
            // No saved preference: keep subs off by default (matches
            // the previous behaviour — the source `default` flag is
            // surfaced via the gear menu but we don't auto-enable it).
            params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
        }
        player.trackSelectionParameters = params.build()
    }

    // Track the user's current audio + subtitle picks so subsequent
    // `saveProgress` calls can persist them. Initialized to the saved
    // stream indices so the first `onTracksChanged` event (which
    // happens BEFORE any user interaction, when Media3 first applies
    // our preferences) matches and triggers no spurious save.
    val currentAudioIdxRef = remember { java.util.concurrent.atomic.AtomicReference<Int?>(initialAudioIdx) }
    val currentSubIdxRef = remember { java.util.concurrent.atomic.AtomicReference<Int?>(initialSubIdx) }
    // Latched once the first non-empty Tracks event arrives so we
    // know to (a) apply the `TrackSelectionOverride` to pin the saved
    // pick, and (b) swallow that first event without saving — the
    // user hasn't touched anything yet.
    val initialRestoreDone = remember {
        java.util.concurrent.atomic.AtomicBoolean(false)
    }
    // Persist track-pick changes IMMEDIATELY (debounced 500 ms to
    // coalesce rapid clicks). The tick-based save inside the
    // playback loop only fires every 7 s, and the `onDispose` save
    // races a `rememberCoroutineScope` cancellation if the user
    // back-presses straight after picking — both windows allowed
    // an "I picked English audio then backed out" change to be
    // dropped silently. Firing here on every `onTracksChanged`
    // closes that gap. We use `container.applicationScope` so the
    // POST survives the composable unmount; the debounce job is
    // tracked locally so a fresh pick cancels the pending save.
    val pendingTrackSaveJob = remember {
        java.util.concurrent.atomic.AtomicReference<kotlinx.coroutines.Job?>(null)
    }
    val scheduleTrackSave = remember(serverUrl, infohash, fileIdx, player) {
        {
            pendingTrackSaveJob.getAndSet(null)?.cancel()
            val job = container.applicationScope.launch {
                kotlinx.coroutines.delay(500)
                // Media3's `Player` is single-threaded — every getter
                // / setter MUST be called from the player's
                // application looper (the main thread, in our case).
                // `applicationScope` is `Dispatchers.IO`, so we hop
                // to Main just for the property reads, then return to
                // IO for the network POST. Without the hop the player
                // throws `IllegalStateException("Player is accessed
                // on the wrong thread")` and the audio-track switch
                // crashes the app.
                val (pos, dur) = kotlinx.coroutines.withContext(
                    kotlinx.coroutines.Dispatchers.Main,
                ) {
                    player.currentPosition to player.duration.takeIf { it > 0 }
                }
                runCatching {
                    container.apiFor(serverUrl).saveProgress(
                        infohash = infohash,
                        idx = fileIdx,
                        body = ProgressUpdate(
                            positionSeconds = pos / 1000.0,
                            durationSeconds = dur?.div(1000.0),
                            audioTrackIdx = currentAudioIdxRef.get(),
                            subtitleTrackIdx = currentSubIdxRef.get(),
                            completed = dur != null && pos >= dur - 30_000,
                        ),
                    )
                }
            }
            pendingTrackSaveJob.set(job)
        }
    }

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
                when (state) {
                    androidx.media3.common.Player.STATE_READY -> {
                        // Successful resume — reset the retry budget
                        // AND clear any stale ENDED flag (a buffer
                        // underrun followed by re-prepare looks like
                        // a brief STATE_ENDED → STATE_READY blip; we
                        // don't want that to fire the auto-advance
                        // countdown while playback is actually
                        // continuing).
                        retryCount = 0
                        firstRetryAt = 0L
                        playerEnded = false
                    }
                    androidx.media3.common.Player.STATE_ENDED -> {
                        playerEnded = true
                    }
                }
            }

            override fun onTracksChanged(tracks: androidx.media3.common.Tracks) {
                // Slice the Tracks bundle into audio + text groups in
                // source order. For MKV / MP4 each track surfaces as
                // its own `Tracks.Group` (length=1) and the order
                // matches `probe.audio` / `probe.subtitle` — so the
                // N-th audio group ↔ `probe.audio[N]`, and we can
                // identify tracks by ordinal instead of language.
                val audioGroups = tracks.groups.filter { it.type == C.TRACK_TYPE_AUDIO }
                val subGroups = tracks.groups.filter { it.type == C.TRACK_TYPE_TEXT }

                // First event with real tracks loaded: pin the saved
                // pick via `TrackSelectionOverride` (the only way to
                // address a specific track group from outside the
                // gear menu) and swallow the save — the user hasn't
                // touched anything yet, so writing now would clobber
                // the value we just read back from the server.
                if (!initialRestoreDone.get() && audioGroups.isNotEmpty()) {
                    initialRestoreDone.set(true)
                    val params = player.trackSelectionParameters.buildUpon()
                    var dirty = false
                    if (
                        savedAudioOrdinal != null &&
                        savedAudioOrdinal in audioGroups.indices
                    ) {
                        val currentSelected = audioGroups.indexOfFirst { it.isSelected }
                        if (currentSelected != savedAudioOrdinal) {
                            val target = audioGroups[savedAudioOrdinal]
                            params.setOverrideForType(
                                androidx.media3.common.TrackSelectionOverride(
                                    target.mediaTrackGroup, 0,
                                ),
                            )
                            dirty = true
                        }
                    }
                    when (savedSubOrdinal) {
                        -1 -> { /* already disabled via LaunchedEffect */ }
                        null -> { /* no preference */ }
                        else -> if (savedSubOrdinal in subGroups.indices) {
                            val currentSelected = subGroups.indexOfFirst { it.isSelected }
                            if (currentSelected != savedSubOrdinal) {
                                val target = subGroups[savedSubOrdinal]
                                params
                                    .setOverrideForType(
                                        androidx.media3.common.TrackSelectionOverride(
                                            target.mediaTrackGroup, 0,
                                        ),
                                    )
                                    .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                                dirty = true
                            }
                        }
                    }
                    if (dirty) player.trackSelectionParameters = params.build()
                    return
                }

                // Event-driven save. Map the currently-selected group
                // back to a `probe.audio[i].index` by its ordinal in
                // the audio-groups list. `-1` for subs means the
                // user has subtitles disabled.
                val pickedAudioOrdinal = audioGroups.indexOfFirst { it.isSelected }
                val newAudioIdx =
                    if (pickedAudioOrdinal >= 0)
                        probe.audio.getOrNull(pickedAudioOrdinal)?.index ?: currentAudioIdxRef.get()
                    else currentAudioIdxRef.get()
                val pickedSubOrdinal = subGroups.indexOfFirst { it.isSelected }
                val newSubIdx: Int? =
                    if (pickedSubOrdinal >= 0)
                        probe.subtitle.getOrNull(pickedSubOrdinal)?.index ?: currentSubIdxRef.get()
                    else -1

                val audioChanged = currentAudioIdxRef.getAndSet(newAudioIdx) != newAudioIdx
                val subChanged = currentSubIdxRef.getAndSet(newSubIdx) != newSubIdx
                if (audioChanged || subChanged) {
                    // Debounced save so the new pick survives an
                    // immediate back-press — see `scheduleTrackSave`.
                    scheduleTrackSave()
                }
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
                    if (
                        !nearEnd
                        && durationMs > 0
                        && pos.toFloat() / durationMs.toFloat() >= 0.95f
                    ) {
                        // Fire the bigger credit-roll pill once. The
                        // top-right chips have been visible all along;
                        // this is the prominent nudge.
                        nearEnd = true
                    }
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
            // Cancel any pending debounced track-save so we don't
            // race the dispose-time POST with a stale 500 ms-old one.
            pendingTrackSaveJob.getAndSet(null)?.cancel()
            // Use the process-lifetime scope here — `rememberCoroutineScope`
            // is being cancelled as the composable unmounts, which on
            // some back-press flows dropped the final save before the
            // POST left the device.
            container.applicationScope.launch {
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

    // Captured for the BackHandler below so the first back-press can
    // dismiss the controller overlay before the second one actually
    // leaves the screen.
    var playerView by remember { mutableStateOf<PlayerView?>(null) }
    // Pushed by PlayerView's `ControllerVisibilityListener` whenever the
    // play/pause overlay shows or hides. Drives the top-right nav chips
    // — they only make sense alongside the rest of the chrome, so we
    // gate them on the same signal. Starts `false` so the chips stay
    // hidden on entry; the listener flips it to `true` if/when the
    // controller comes up.
    var controllerVisible by remember { mutableStateOf(false) }

    // Intercept the back gesture: if the controller overlay is
    // currently showing, hide it instead of leaving the watch
    // screen. Falls through to the default back behaviour (return
    // to home) only once the overlay is already invisible — so the
    // user gets the standard "one back to dismiss, one back to
    // exit" two-step instead of being kicked all the way out.
    // `controllerAutoShow = true` makes the overlay come up on the
    // first remote click anyway, so this matters on every session.
    androidx.activity.compose.BackHandler(
        enabled = playerView?.isControllerFullyVisible == true,
    ) {
        playerView?.hideController()
    }

    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { ctx ->
            PlayerView(ctx).apply {
                // Register the visibility listener BEFORE attaching the
                // player. Once the player is set, Media3 may fire the
                // initial `controllerAutoShow` synchronously — anything
                // we register afterwards would miss that first event
                // and the chips would sit stuck at their default value
                // until the user wiggled the controller manually.
                setControllerVisibilityListener(
                    PlayerView.ControllerVisibilityListener { visibility ->
                        controllerVisible = visibility == android.view.View.VISIBLE
                    },
                )
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
                // Sync the current value — defensive belt-and-braces for
                // the rare case the platform raced past the listener
                // registration on slower devices.
                controllerVisible = isControllerFullyVisible
                playerView = this
            }
        },
        update = { it.player = player },
    )

    // Always-on episode-navigation overlay. Two small chips in the
    // top-right corner of the player surface: "‹ Prev" and "Next ›",
    // each appearing only when the backend resolved an episode in
    // that direction. The chips never auto-advance — every action is
    // a deliberate D-pad click. Subtle enough to ignore during the
    // show, discoverable enough to reach for at the credits.
    //
    // Exception: when the player has reached STATE_ENDED AND there's
    // no next episode at all, swap the next chip for a more visible
    // "End of series — Back to Home" banner so the user has a clear
    // exit path instead of staring at a frozen final frame.
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 32.dp, vertical = 24.dp),
    ) {
        // Top chips ride with the rest of the chrome: visible while
        // PlayerView's controller overlay is up, or once we're near
        // the end of the episode (≥ 95 %) or after it has ended —
        // those two moments are when next-episode navigation is the
        // most relevant action and the user shouldn't have to wake
        // the controller first.
        val chipsVisible = controllerVisible || nearEnd || playerEnded
        if (chipsVisible) {
            Row(
                modifier = Modifier.align(Alignment.TopEnd),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                prevEpisode?.let { prev ->
                EpisodeNavChip(
                    label = "S%02dE%02d".format(prev.season, prev.episode),
                    direction = NavDirection.Prev,
                    grabbing = grabbing && prev.status == "available",
                    point = prev,
                    onPlay = {
                        val ih = prev.infohash
                        val idx = prev.fileIdx
                        if (ih != null && idx != null) onNavigateToFile(ih, idx)
                    },
                    onPrepare = {
                        val fid = prev.followId ?: return@EpisodeNavChip
                        grabbing = true
                        scope.launch {
                            val grabbed = runCatching {
                                container.apiFor(serverUrl).grabEpisode(
                                    id = fid,
                                    season = prev.season,
                                    episode = prev.episode,
                                )
                            }.getOrNull()
                            grabbing = false
                            if (grabbed != null) {
                                prevEpisode = prev.copy(
                                    status = "downloaded",
                                    infohash = grabbed.infohash,
                                    fileIdx = grabbed.fileIdx,
                                )
                            }
                        }
                    },
                )
            }
            nextEpisode?.let { next ->
                EpisodeNavChip(
                    label = "S%02dE%02d".format(next.season, next.episode),
                    direction = NavDirection.Next,
                    grabbing = grabbing && next.status == "available",
                    point = next,
                    onPlay = {
                        val ih = next.infohash
                        val idx = next.fileIdx
                        if (ih != null && idx != null) onNavigateToFile(ih, idx)
                    },
                    onPrepare = {
                        val fid = next.followId ?: return@EpisodeNavChip
                        grabbing = true
                        scope.launch {
                            val grabbed = runCatching {
                                container.apiFor(serverUrl).grabEpisode(
                                    id = fid,
                                    season = next.season,
                                    episode = next.episode,
                                )
                            }.getOrNull()
                            grabbing = false
                            if (grabbed != null) {
                                nextEpisode = next.copy(
                                    status = "downloaded",
                                    infohash = grabbed.infohash,
                                    fileIdx = grabbed.fileIdx,
                                )
                            }
                        }
                    },
                )
            }
            }
        }

        if (playerEnded && nextEpisode == null) {
            Box(
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(bottom = 96.dp),
            ) {
                EndOfSeriesPill(onBack = onBack)
            }
        } else if ((nearEnd || playerEnded) && !pillDismissed) {
            // Bigger Netflix-style "Up next" pill in the bottom-right
            // corner, appearing once we're past 95 % of the runtime.
            // No countdown, no auto-advance — a single deliberate
            // "Play next" tap (or Dismiss to silence it).
            val next = nextEpisode
            if (next != null) {
                Box(
                    modifier = Modifier
                        .align(Alignment.BottomEnd)
                        .padding(bottom = 96.dp),
                ) {
                    UpNextPill(
                        next = next,
                        grabbing = grabbing && next.status == "available",
                        onPlay = {
                            val ih = next.infohash
                            val idx = next.fileIdx
                            if (ih != null && idx != null) onNavigateToFile(ih, idx)
                        },
                        onPrepare = {
                            val fid = next.followId ?: return@UpNextPill
                            grabbing = true
                            scope.launch {
                                val grabbed = runCatching {
                                    container.apiFor(serverUrl).grabEpisode(
                                        id = fid,
                                        season = next.season,
                                        episode = next.episode,
                                    )
                                }.getOrNull()
                                grabbing = false
                                if (grabbed != null) {
                                    nextEpisode = next.copy(
                                        status = "downloaded",
                                        infohash = grabbed.infohash,
                                        fileIdx = grabbed.fileIdx,
                                    )
                                }
                            }
                        },
                        onDismiss = { pillDismissed = true },
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun UpNextPill(
    next: EpisodePoint,
    grabbing: Boolean,
    onPlay: () -> Unit,
    onPrepare: () -> Unit,
    onDismiss: () -> Unit,
) {
    val downloaded = next.status == "downloaded" && next.infohash != null && next.fileIdx != null
    Surface(
        shape = RoundedCornerShape(12.dp),
        colors = SurfaceDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.92f),
        ),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 20.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                "Up next".uppercase(),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "S%02dE%02d".format(next.season, next.episode),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.padding(top = 4.dp),
            ) {
                if (downloaded) {
                    Button(
                        onClick = onPlay,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    ) { Text("Play next") }
                } else {
                    Button(
                        onClick = onPrepare,
                        enabled = !grabbing && next.followId != null,
                        shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    ) { Text(if (grabbing) "Preparing…" else "Prepare") }
                }
                Button(
                    onClick = onDismiss,
                    shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                ) { Text("Dismiss") }
            }
        }
    }
}

private enum class NavDirection { Prev, Next }

/**
 * Small focusable chip overlaid on the player. "Prev" → left arrow
 * prefix, "Next" → right arrow suffix. Surface is semi-transparent
 * so it doesn't pull focus from the playing video; the focus ring
 * (TV-Material's default) makes it obvious where the D-pad is. For
 * downloaded episodes a tap navigates immediately; for available
 * ones it kicks off a `/grab` and the chip flips to "Preparing…",
 * then once the grab returns the parent re-renders us with
 * `status == "downloaded"`.
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EpisodeNavChip(
    label: String,
    direction: NavDirection,
    grabbing: Boolean,
    point: EpisodePoint,
    onPlay: () -> Unit,
    onPrepare: () -> Unit,
) {
    val downloaded = point.status == "downloaded" && point.infohash != null && point.fileIdx != null
    val text = when {
        grabbing -> "Preparing $label…"
        downloaded && direction == NavDirection.Prev -> "‹ $label"
        downloaded && direction == NavDirection.Next -> "$label ›"
        direction == NavDirection.Prev -> "‹ Prepare $label"
        else -> "Prepare $label ›"
    }
    Button(
        onClick = {
            when {
                grabbing -> Unit
                downloaded -> onPlay()
                else -> onPrepare()
            }
        },
        enabled = !grabbing && (downloaded || point.followId != null),
        shape = ButtonDefaults.shape(shape = RoundedCornerShape(20.dp)),
        contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp),
        colors = ButtonDefaults.colors(
            // Translucent so the chip reads as overlay, not chrome —
            // matches the visual weight of subtitle blocks rather
            // than the bottom controller bar.
            containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.55f),
            contentColor = MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Text(text, style = MaterialTheme.typography.labelMedium)
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EndOfSeriesPill(onBack: () -> Unit) {
    Surface(
        shape = RoundedCornerShape(12.dp),
        colors = SurfaceDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.92f),
        ),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 20.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                "You're all caught up".uppercase(),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "No more episodes available.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Button(
                onClick = onBack,
                shape = ButtonDefaults.shape(shape = RoundedCornerShape(8.dp)),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                modifier = Modifier.padding(top = 4.dp),
            ) { Text("Back to Home") }
        }
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
