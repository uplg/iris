// Media3's Player, Tracks and MediaItem APIs are `@UnstableApi`, marked with
// `androidx.annotation.RequiresOptIn`: the file-level androidx OptIn satisfies
// lint, and kotlinc needs none.
@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package studio.kahn.iris.tv.ui.screens.player

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.C
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.Tracks
import androidx.media3.exoplayer.ExoPlayer
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import studio.kahn.iris.tv.data.AppContainer
import studio.kahn.iris.tv.data.IrisCaps
import studio.kahn.iris.tv.data.PlayStatus
import studio.kahn.iris.tv.data.SeekHint
import studio.kahn.iris.tv.data.SubtitlePick
import studio.kahn.iris.tv.data.UpdatePlaybackPrefs
import studio.kahn.iris.tv.data.bestEffort
import studio.kahn.iris.tv.data.buildMediaItem
import studio.kahn.iris.tv.data.buildPlayer
import studio.kahn.iris.tv.data.humanizePlaybackError
import studio.kahn.iris.tv.data.isRemuxableError
import studio.kahn.iris.tv.data.serverBase
import studio.kahn.iris.tv.data.webVttSubtitle
import studio.kahn.iris.tv.ui.components.buildMediaSession
import studio.kahn.iris.tv.ui.format.NO_SUBTITLES
import studio.kahn.iris.tv.ui.state.RepeatWhileStarted

/**
 * What the player UI reads from [VodEngine]: published once per change, so
 * only the composables that read a field recompose.
 */
@Stable
class VodPlayback {
    var player: ExoPlayer? by mutableStateOf(null)
        internal set
    var route by mutableStateOf(PlayRoute.Direct)
        internal set
    /** The server builds the stream first, and playback waits for it. */
    var serverPrep by mutableStateOf(false)
        internal set
    var serverReady by mutableStateOf(false)
        internal set
    var serverStatus by mutableStateOf<PlayStatus?>(null)
        internal set
    /** A real frame is on screen: getting ready is over. */
    var firstFrameRendered by mutableStateOf(false)
        internal set
    var ended by mutableStateOf(false)
        internal set
    /** Crossed 95 % at least once (the next-episode rule). */
    var nearEnd by mutableStateOf(false)
        internal set
    /** A playback error the player could not get past, in words. */
    var error by mutableStateOf<String?>(null)
        internal set
    var tracks by mutableStateOf(Tracks.EMPTY)
        internal set
    /** The film's duration from the probe (a growing server stream reports less), 0 when unknown. */
    var filmDurationMs by mutableLongStateOf(0L)
        internal set
    /**
     * The picture stopped reaching anyone (`OnOutputLost`), maybe before the player existed:
     * nothing starts playing by itself until the viewer presses Play.
     */
    var outputLost by mutableStateOf(false)

    /** Re-prepare after an error ("Try again"). */
    fun retry() {
        error = null
        player?.prepare()
    }

    /** A new player starts its tracks afresh. */
    internal fun resetTracks() {
        tracks = Tracks.EMPTY
    }
}

/**
 * The playback engine of one file: builds the Media3 player for the right
 * stream, starts it at the resume point, restores and saves the track picks,
 * reports progress ([ProgressSaver]), falls back to the server's HLS remux
 * when the device cannot decode the file, stops when the screen stops and
 * pauses when the picture goes away. No UI: it publishes into [out].
 */
@Composable
fun VodEngine(
    container: AppContainer,
    vm: WatchViewModel,
    setup: WatchSetup,
    title: String,
    out: VodPlayback,
) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val scope = rememberCoroutineScope()
    val infohash = vm.infohash
    val fileIdx = vm.fileIdx
    val serverUrl = setup.serverUrl
    val probe = setup.probe
    val prefAudioLang = setup.prefAudioLang
    val prefSubLang = setup.prefSubLang
    val currentTitle by rememberUpdatedState(title)
    val fileSizeBytes by vm.fileSizeBytes.collectAsStateWithLifecycle()
    val currentFileSize by rememberUpdatedState(fileSizeBytes)

    // Tier-F fallback gate: flipped by the error listener when ExoPlayer chokes
    // on the container / decoder / codec (DV, Atmos JOC, exotic HEVC profiles…).
    // One-shot per file.
    var useRemuxFallback by remember(infohash, fileIdx) { mutableStateOf(false) }
    // Position at the moment the fallback fires, so the HLS player resumes there.
    var fallbackResumeMs by remember(infohash, fileIdx) { mutableLongStateOf(0L) }
    // The last position the progress tick saw (the resume point of a rebuilt player). Kept by
    // the ViewModel too, so an activity recreated mid-film resumes there, not where it opened.
    val lastPositionMs = remember(infohash, fileIdx) { AtomicLong(vm.playedMs ?: (setup.resumeSec * 1000).toLong()) }

    // Per-stream AV1 decode routing, from the probed codec + bit depth. 8-bit
    // AV1 hardware-decodes on ANY AV1 silicon; 10-bit additionally needs the
    // decoder to declare Main10. Must stay in lockstep with the `Iris-Caps`
    // header and `buildPlayer`'s renderer ordering (see `IrisCaps`).
    val probedAv1 = remember(probe) {
        probe.video.firstOrNull()?.takeIf { it.codec.equals("av1", ignoreCase = true) }
    }
    val av1HardwareFits = remember(probedAv1) {
        when {
            probedAv1 == null -> false
            (probedAv1.bitDepth ?: 8) >= 10 -> IrisCaps.hardwareAv1Main10
            else -> IrisCaps.hasHardwareDecoder("av1")
        }
    }
    // A 10-bit AV1 file on silicon that can't take it plays the server's
    // re-encode from the start (mirrors the server's `decide_video_mode`).
    val needsServerTranscode = remember(probedAv1, av1HardwareFits) {
        probedAv1 != null && (probedAv1.bitDepth ?: 8) >= 10 && !av1HardwareFits
    }
    val route = when {
        useRemuxFallback -> PlayRoute.ServerRemux
        needsServerTranscode -> PlayRoute.ServerTranscode
        else -> PlayRoute.Direct
    }
    // The player's track groups in probe terms on this route: picks are ordinals into these.
    val routeTracks = remember(probe, route) { RouteTracks.of(probe, route) }

    val playUrl = remember(serverUrl, infohash, fileIdx, route) {
        val base = serverBase(serverUrl)
        if (route != PlayRoute.Direct) {
            "${base}api/torrents/$infohash/files/$fileIdx/play/master.m3u8"
        } else {
            "${base}api/torrents/$infohash/files/$fileIdx/stream"
        }
    }

    // Re-armed per stream: the loader stays until the player paints a frame.
    var firstFrameRendered by remember(playUrl) { mutableStateOf(false) }
    LaunchedEffect(playUrl) {
        out.firstFrameRendered = false
        out.serverStatus = null
        out.error = null
    }

    val resumeMs = remember(playUrl) {
        maxOf(lastPositionMs.get(), fallbackResumeMs).coerceAtLeast(0)
    }

    // ffprobe's duration: a growing HLS EVENT transcode reports only the
    // encoder's edge as `player.duration`.
    val filmDurationMs = remember(probe) {
        ((probe.durationSeconds ?: 0.0) * 1000).toLong()
    }

    // Both server paths stream a GROWING HLS EVENT playlist: don't prepare
    // before the encoder has passed the resume point (+10 s), or the seek
    // waits on the encoder past the player's IO timeout and the player dies.
    // The reactive remux is always gated (its cold start is what failed).
    val gateOnServerBuild = useRemuxFallback || (needsServerTranscode && resumeMs > 0)
    var portionReady by remember(playUrl) { mutableStateOf(!gateOnServerBuild) }
    if (route != PlayRoute.Direct) {
        // Only while the screen is started: the status read also kicks the server's build.
        RepeatWhileStarted(playUrl) {
            val durationSec = probe.durationSeconds ?: 0.0
            val resumeSec = resumeMs / 1000.0
            vm.pollPlayStatus(keepGoing = { !firstFrameRendered }) { st ->
                out.serverStatus = st
                if (gateOnServerBuild && !portionReady) {
                    val encodedSec = (st.progress ?: 0.0) * durationSec
                    if (st.ready || durationSec <= 0.0 || encodedSec >= resumeSec + 10.0) {
                        portionReady = true
                    }
                }
            }
        }
    }

    // The server HLS master carries no subtitle renditions: side-load each
    // text-based source sub as WebVTT, in [RouteTracks]' order.
    val sideLoadedSubs = remember(routeTracks, serverUrl, infohash, fileIdx, route) {
        if (route == PlayRoute.Direct) {
            emptyList()
        } else {
            val base = serverBase(serverUrl)
            routeTracks.subtitles.map { s ->
                webVttSubtitle(
                    url = "${base}api/torrents/$infohash/files/$fileIdx/sub/${s.absoluteIndex}/track.vtt",
                    language = s.language,
                    label = s.title ?: s.language,
                    forced = s.forced,
                )
            }
        }
    }

    // `preferPlatformAv1` only matters on the direct path: the server streams
    // carry H.264/HEVC, hardware-decoded under either renderer order.
    val player = remember(playUrl, av1HardwareFits) {
        buildPlayer(
            context,
            container.mediaOkHttpClient,
            preferPlatformAv1 = av1HardwareFits,
        )
    }

    // The one way progress reaches the server; it outlives a rebuilt player (the remux fallback).
    val saver = remember(infohash, fileIdx) {
        ProgressSaver(lastPositionMs.get()) { body, failed ->
            container.applicationScope.launch {
                bestEffort { container.apiFor(serverUrl).saveProgress(infohash = infohash, idx = fileIdx, body = body) }
                    ?: failed()
            }
        }.apply {
            audioIdx = setup.savedAudioIdx
            subtitleIdx = setup.savedSubIdx
        }
    }
    val durationOf: () -> Long? = { filmDurationMs.takeIf { it > 0 } ?: player.duration.takeIf { it > 0 } }

    LaunchedEffect(player) { out.resetTracks() }
    LaunchedEffect(player, portionReady) {
        if (!portionReady) return@LaunchedEffect
        // Never starts behind Home or the screensaver: it waits for the screen.
        lifecycle.currentStateFlow.first { it.isAtLeast(Lifecycle.State.STARTED) }
        player.setMediaItem(buildMediaItem(playUrl, currentTitle, subtitles = sideLoadedSubs), resumeMs)
        player.prepare()
        player.playWhenReady = !out.outputLost
    }

    // The picks to restore on this player: the viewer's current ones (seeded with the saved
    // ones), as ordinals into this route's groups. Subtitles: -1 = turned off, null = no pick.
    val pinAudioOrdinal = remember(player, routeTracks) { routeTracks.audioOrdinal(saver.audioIdx) }
    val pinSubIdx = remember(player) { saver.subtitleIdx }
    val pinSubOrdinal: Int? = remember(player, routeTracks) { routeTracks.subtitleOrdinal(pinSubIdx) }
    // No per-file pick: the track the preferred language maps to (non-forced
    // before forced, plain before SDH: `SubtitlePick`).
    val preferredSubOrdinal: Int? = remember(player, routeTracks, prefSubLang) {
        if (pinSubIdx != null || prefSubLang == NO_SUBTITLES) null
        else SubtitlePick.preferredOrdinal(routeTracks.subtitles, prefSubLang)
    }

    // A language hint at load time so the first frames play the right audio;
    // the exact pin is the override applied on the first onTracksChanged.
    LaunchedEffect(player, routeTracks, prefAudioLang, prefSubLang) {
        val initialAudio = pinAudioOrdinal?.let { routeTracks.audio[it].language }
            ?: prefAudioLang
            ?: probe.audio.firstOrNull { it.default }?.language
            ?: probe.audio.firstOrNull()?.language
        val pinSubLang = pinSubOrdinal
            ?.takeIf { it >= 0 && it in routeTracks.subtitles.indices }
            ?.let { routeTracks.subtitles[it].language }
        val params = player.trackSelectionParameters.buildUpon()
        if (initialAudio != null) params.setPreferredAudioLanguage(initialAudio)
        when {
            pinSubLang != null -> {
                params.setPreferredTextLanguage(pinSubLang)
                params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
            }
            pinSubIdx == -1 -> params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
            prefSubLang == NO_SUBTITLES -> params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
            // Enable only when the preferred language is present: never force a
            // different language onto the viewer.
            preferredSubOrdinal != null -> {
                params.setPreferredTextLanguage(prefSubLang)
                params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
            }
            else -> params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
        }
        player.trackSelectionParameters = params.build()
    }

    // Per player: its first tracks event is Media3 applying the picks above, not the viewer.
    val initialRestoreDone = remember(player) { AtomicBoolean(false) }
    // A pick is saved at once (500 ms debounce), from the application scope so
    // a Back right after the pick doesn't drop it.
    val pendingTrackSaveJob = remember { AtomicReference<Job?>(null) }
    val collectionId = setup.collectionId
    val savePrefs: suspend () -> Unit = {
        // The chosen LANGUAGES too, for the next episode and device: the
        // series' choice when the file belongs to one (the web's rule).
        val audioLang = routeTracks.audioLanguage(saver.audioIdx)
        val subLang = routeTracks.subtitleLanguage(saver.subtitleIdx)
        bestEffort {
            container.apiFor(serverUrl).savePlaybackPreferences(
                UpdatePlaybackPrefs(audioLanguage = audioLang, collectionId = collectionId, subtitleLanguage = subLang),
            )
        }
    }
    val scheduleTrackSave = remember(player, routeTracks) {
        {
            pendingTrackSaveJob.getAndSet(null)?.cancel()
            val job = container.applicationScope.launch {
                delay(500)
                // Player getters on its looper (main).
                withContext(Dispatchers.Main) { saver.save(player.currentPosition, durationOf(), playing = player.isPlaying) }
                savePrefs()
            }
            pendingTrackSaveJob.set(job)
        }
    }

    // Seek hint: on every user seek, ask the server to prioritise ~30 s of
    // bytes past the new playhead (byte offset ≈ playhead × size / duration).
    // A pause says so at once (the admin "Now watching" shows it).
    DisposableEffect(player) {
        val listener = object : Player.Listener {
            override fun onPositionDiscontinuity(
                oldPosition: Player.PositionInfo,
                newPosition: Player.PositionInfo,
                reason: Int,
            ) {
                if (reason != Player.DISCONTINUITY_REASON_SEEK) return
                saver.seeked()
                val size = currentFileSize
                val durMs = filmDurationMs.takeIf { it > 0 } ?: player.duration
                if (durMs <= 0 || size <= 0) return
                val playheadS = newPosition.positionMs / 1000.0
                val byteOffset = ((newPosition.positionMs.toDouble() / durMs.toDouble()) * size)
                    .toLong().coerceIn(0L, size - 1)
                container.applicationScope.launch {
                    bestEffort {
                        container.apiFor(serverUrl).postSeekHint(
                            infohash = infohash,
                            idx = fileIdx,
                            body = SeekHint(byteOffset = byteOffset, playheadS = playheadS),
                        )
                    }
                }
            }

            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                if (playWhenReady) {
                    out.outputLost = false
                } else if (player.playbackState != Player.STATE_IDLE) {
                    saver.save(player.currentPosition, durationOf(), playing = false)
                }
            }
        }
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
    }

    // Errors: transient ones re-prepare up to 3 times in 30 s; codec /
    // container / decoder ones switch once to the server remux; the rest are
    // said to the viewer.
    DisposableEffect(player) {
        val maxRetries = 3
        val retryWindowMs = 30_000L
        var retryCount = 0
        var firstRetryAt = 0L
        val listener = object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                val (message, transient) = humanizePlaybackError(error)
                Log.w("iris-core", "playback error ${error.errorCodeName} transient=$transient: ${error.message}")
                val now = SystemClock.uptimeMillis()
                if (firstRetryAt == 0L || now - firstRetryAt > retryWindowMs) {
                    firstRetryAt = now
                    retryCount = 0
                }
                if (transient && retryCount < maxRetries) {
                    retryCount++
                    Log.i("iris-core", "auto-retry #$retryCount after transient error")
                    scope.launch {
                        delay(1_500L * retryCount)
                        bestEffort { player.prepare() }
                    }
                    return
                }
                if (!useRemuxFallback && isRemuxableError(error)) {
                    Log.i("iris-core", "switching to server-side HLS remux after ${error.errorCodeName}")
                    fallbackResumeMs = player.currentPosition.coerceAtLeast(0L)
                    useRemuxFallback = true
                    return
                }
                out.error = message
            }

            override fun onRenderedFirstFrame() {
                firstFrameRendered = true
                out.firstFrameRendered = true
            }

            override fun onPlaybackStateChanged(state: Int) {
                when (state) {
                    Player.STATE_READY -> {
                        // A buffer underrun + re-prepare blips through ENDED: clear it.
                        retryCount = 0
                        firstRetryAt = 0L
                        out.ended = false
                        out.error = null
                    }
                    Player.STATE_ENDED -> out.ended = true
                    else -> Unit
                }
            }

            override fun onTracksChanged(tracks: Tracks) {
                onTracks(tracks)
            }

            private fun onTracks(newTracks: Tracks) {
                out.tracks = newTracks
                val audioGroups = newTracks.groups.filter { it.type == C.TRACK_TYPE_AUDIO }
                val subGroups = newTracks.groups.filter { it.type == C.TRACK_TYPE_TEXT }

                // Direct-play audio completeness telemetry: MatroskaExtractor
                // drops tracks whose CodecID it doesn't know.
                if (
                    route == PlayRoute.Direct &&
                    newTracks.groups.isNotEmpty() &&
                    (audioGroups.size < probe.audio.size || audioGroups.any { !it.isSupported })
                ) {
                    Log.w(
                        "iris-core",
                        "direct-play audio deficit: ${audioGroups.size} surfaced " +
                            "(${audioGroups.count { it.isSupported }} supported) of ${probe.audio.size} probed",
                    )
                }

                // First event with real tracks: pin the picks (or the
                // preference's) and save nothing, the viewer touched nothing.
                if (!initialRestoreDone.get() && audioGroups.isNotEmpty()) {
                    initialRestoreDone.set(true)
                    val params = player.trackSelectionParameters.buildUpon()
                    var dirty = false
                    var settledSubOrdinal = subGroups.indexOfFirst { it.isSelected }
                    if (pinAudioOrdinal != null && pinAudioOrdinal in audioGroups.indices) {
                        val currentSelected = audioGroups.indexOfFirst { it.isSelected }
                        if (currentSelected != pinAudioOrdinal) {
                            params.setOverrideForType(TrackSelectionOverride(audioGroups[pinAudioOrdinal].mediaTrackGroup, 0))
                            dirty = true
                        }
                    }
                    val pinSub = when (pinSubOrdinal) {
                        -1 -> null
                        null -> preferredSubOrdinal
                        else -> pinSubOrdinal
                    }
                    if (pinSub != null && pinSub in subGroups.indices) {
                        settledSubOrdinal = pinSub
                        if (subGroups.indexOfFirst { it.isSelected } != pinSub) {
                            params
                                .setOverrideForType(TrackSelectionOverride(subGroups[pinSub].mediaTrackGroup, 0))
                                .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
                            dirty = true
                        }
                    }
                    // Seed the picks with the settled state, or the next event
                    // reads "no subtitle" as a change and saves an "off" nobody chose.
                    val settledAudioOrdinal = pinAudioOrdinal?.takeIf { it in audioGroups.indices }
                        ?: audioGroups.indexOfFirst { it.isSelected }
                    saver.audioIdx = routeTracks.audioIndexAt(settledAudioOrdinal, audioGroups.size) ?: saver.audioIdx
                    saver.subtitleIdx = if (settledSubOrdinal >= 0) {
                        routeTracks.subtitleIndexAt(settledSubOrdinal, subGroups.size) ?: saver.subtitleIdx
                    } else {
                        -1
                    }
                    if (dirty) player.trackSelectionParameters = params.build()
                    return
                }

                val pickedAudioOrdinal = audioGroups.indexOfFirst { it.isSelected }
                val newAudioIdx = if (pickedAudioOrdinal >= 0) {
                    routeTracks.audioIndexAt(pickedAudioOrdinal, audioGroups.size) ?: saver.audioIdx
                } else {
                    saver.audioIdx
                }
                val pickedSubOrdinal = subGroups.indexOfFirst { it.isSelected }
                val newSubIdx: Int? = if (pickedSubOrdinal >= 0) {
                    routeTracks.subtitleIndexAt(pickedSubOrdinal, subGroups.size) ?: saver.subtitleIdx
                } else {
                    -1
                }
                val changed = saver.audioIdx != newAudioIdx || saver.subtitleIdx != newSubIdx
                saver.audioIdx = newAudioIdx
                saver.subtitleIdx = newSubIdx
                if (changed) scheduleTrackSave()
            }
        }
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
    }

    // The progress tick (1 s) feeds the heartbeat; the last save, the
    // MediaSession and the player go with the stream.
    DisposableEffect(player) {
        val session = buildMediaSession(context, player, "vod")
        val handler = Handler(Looper.getMainLooper())
        var durationMs: Long = filmDurationMs.takeIf { it > 0 } ?: -1
        val tick = object : Runnable {
            override fun run() {
                if (durationMs <= 0 && player.duration > 0) durationMs = player.duration
                val pos = player.currentPosition
                if (pos > 0) {
                    lastPositionMs.set(pos)
                    vm.playedMs = pos
                    if (!out.nearEnd && durationMs > 0 && isNearEnd(pos, durationMs)) out.nearEnd = true
                }
                saver.tick(pos, durationMs.takeIf { it > 0 }, playing = player.isPlaying)
                handler.postDelayed(this, 1_000)
            }
        }
        handler.postDelayed(tick, 1_000)

        onDispose {
            handler.removeCallbacksAndMessages(null)
            val pos = player.currentPosition
            if (pos > 0) {
                lastPositionMs.set(pos)
                vm.playedMs = pos
            }
            // A pick made just before leaving: its languages are saved now, the picks ride on
            // the last progress save.
            pendingTrackSaveJob.getAndSet(null)?.takeIf { it.isActive }?.let { pending ->
                pending.cancel()
                container.applicationScope.launch { savePrefs() }
            }
            saver.save(pos, durationOf(), playing = false)
            session.release()
            player.release()
        }
    }

    // Home (`ON_STOP`) cuts the stream: the player stops (the decoder and the
    // buffer go back to a small box) and keeps its place; on return it is
    // prepared again, paused there. No auto-resume: the viewer presses Play.
    DisposableEffect(player, lifecycle) {
        var stopped = false
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_STOP -> if (player.playbackState != Player.STATE_IDLE) {
                    player.pause()
                    player.stop()
                    stopped = true
                }
                Lifecycle.Event.ON_START -> if (stopped) {
                    stopped = false
                    player.prepare()
                }
                else -> Unit
            }
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    // The picture stopped reaching anyone (WatchScreen's `OnOutputLost`): pause.
    LaunchedEffect(player, out.outputLost) {
        if (out.outputLost && player.playWhenReady) player.pause()
    }

    SideEffect {
        out.player = player
        out.route = route
        out.serverPrep = gateOnServerBuild
        out.serverReady = portionReady
        out.filmDurationMs = filmDurationMs
    }
}
