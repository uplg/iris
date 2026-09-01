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
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.PlayerView
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
import studio.kahn.iris.tv.data.EpisodeStatus
import studio.kahn.iris.tv.data.EpisodePoint
import studio.kahn.iris.tv.data.IrisApi
import studio.kahn.iris.tv.data.MediaProbe
import studio.kahn.iris.tv.data.TorrentState
import studio.kahn.iris.tv.data.UpdatePlaybackPrefs
import studio.kahn.iris.tv.data.ProgressUpdate
import studio.kahn.iris.tv.data.TorrentView
import studio.kahn.iris.tv.data.buildMediaItem
import studio.kahn.iris.tv.data.buildPlayer
import studio.kahn.iris.tv.data.installIrisTrackNameProvider
import studio.kahn.iris.tv.ui.components.Eyebrow
import studio.kahn.iris.tv.ui.components.IrisButton
import studio.kahn.iris.tv.ui.components.IrisButtonVariant
import studio.kahn.iris.tv.ui.components.IrisPlayerView

/** A title counts as watched/done once playback passes this fraction — the
 *  last ~10 % is credits / recap, so 90 % is "finished" for both movies
 *  (removed from Continue Watching) and episodes (Continue Watching then
 *  advances to the next one). Mirrors the web player. */
private const val WATCHED_FRACTION = 0.90

/** Whether `posMs` into `durMs` counts as watched. */
private fun isWatched(posMs: Long, durMs: Long?): Boolean =
    durMs != null && durMs > 0 && posMs >= durMs * WATCHED_FRACTION

/**
 * Full-screen Media3 PlayerView. Pre-mount we poll `/play/status` so
 * the user sees real download / remux progress instead of a silent
 * black screen, and we only construct the player once the synthetic
 * HLS master playlist is on disk (the segments themselves are produced
 * in the background and long-polled by the HTTP layer when missing).
 *
 * D-pad maps to PlayerView's built-in TV controls (play/pause, seek,
 * subtitle + audio track selection via the settings menu); `IrisPlayerView`
 * adds the centre-key play/pause toggle and the single-layer overlay fade. Multi-audio
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
    // Playback is landscape-only on phones; browsing rotates freely.
    studio.kahn.iris.tv.ui.components.LockLandscape()
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
    // Per-user preferred audio + subtitle LANGUAGE (cross-episode / device).
    // Used only when this file has no per-file saved track (see ReadyPlayer):
    // per-file index wins, else this language pref, else the source default.
    var prefAudioLang by remember { mutableStateOf<String?>(null) }
    var prefSubLang by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var probeVersion by remember { mutableStateOf(0) }
    // Probe 404 = the engine no longer serves this infohash (GC-reclaimed,
    // or a stale reference). Instead of a dead "Probe failed" wall, offer
    // the same "Grab it again" the web shows: `/regrab` re-ingests the
    // release from its recorded provenance — same infohash, so the saved
    // position applies — then the normal probe retry loop takes over.
    var gone by remember(infohash, fileIdx) { mutableStateOf(false) }
    var regrabbing by remember(infohash, fileIdx) { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

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
                val resume = progresses.firstOrNull { it.fileIdx == fileIdx.toLong() }
                    ?.takeUnless { it.completed }?.positionSeconds ?: 0.0
                // Per-file progress carries the audio + subtitle
                // picks; safe to ignore failures (first-time watch).
                val saved = runCatching { api.getProgress(infohash, fileIdx) }
                    .getOrNull()
                // Per-user language preference (cross-episode). Best-effort —
                // an older server without the endpoint just yields null.
                val prefs = runCatching { api.playbackPreferences() }.getOrNull()
                // Commit atomically. The order here matters: write
                // `resumePositionSec` BEFORE `probe` so the very
                // first recomposition that sees `probe != null`
                // already has the right start offset.
                resumePositionSec = resume
                savedAudioIdx = saved?.audioTrackIdx?.toInt()
                savedSubIdx = saved?.subtitleTrackIdx?.toInt()
                prefAudioLang = prefs?.audioLanguage
                prefSubLang = prefs?.subtitleLanguage
                probe = freshProbe
                return@LaunchedEffect
            } catch (e: retrofit2.HttpException) {
                if (e.code() == 404) {
                    gone = true
                    error = "This file is no longer on disk"
                    return@LaunchedEffect
                }
                if (e.code() == 401) {
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
                prefAudioLang = prefAudioLang,
                prefSubLang = prefSubLang,
                onPositionUpdate = { resumePositionSec = it },
                onPlayerError = { error = it },
                onBack = onBack,
                onNavigateToFile = onNavigateToFile,
            )
        } else {
            LoadingOverlay(
                error = error,
                gone = gone,
                regrabbing = regrabbing,
                probeReady = probe != null,
                torrent = torrent,
                onRetry = {
                    probe = null
                    error = null
                    probeVersion++
                },
                onRegrab = {
                    if (!regrabbing) {
                        scope.launch {
                            regrabbing = true
                            val url = container.sessionStore.serverUrl.first()
                            val ok = url != null &&
                                runCatching {
                                    kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                                        container.apiFor(url).regrabTorrent(infohash)
                                    }
                                }.isSuccess
                            regrabbing = false
                            gone = false
                            if (ok) {
                                error = null
                                probe = null
                                probeVersion++
                            } else {
                                error = "Re-grab failed — find this release from its series page"
                            }
                        }
                    }
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
    prefAudioLang: String?,
    prefSubLang: String?,
    onPositionUpdate: (Double) -> Unit,
    onPlayerError: (String) -> Unit,
    onBack: () -> Unit,
    onNavigateToFile: (String, Int) -> Unit,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()

    // Episode navigation context — fetched once per mount. `nextEpisode`
    // drives the native control-bar "Next episode" button; `currentEpisode`
    // picks the finale-vs-movie end copy. Either may be null (movie / last
    // episode).
    var nextEpisode by remember(infohash, fileIdx) {
        mutableStateOf<EpisodePoint?>(null)
    }
    var currentEpisode by remember(infohash, fileIdx) {
        mutableStateOf<EpisodePoint?>(null)
    }
    // Movies have no episode taxonomy, so `currentEpisode` is null — used to
    // pick the series-finale vs. movie-worded end copy, and to suppress any
    // same-torrent "Next episode" affordance (for a movie that'd be a stray
    // extra file).
    val isMovie = currentEpisode == null
    // Player.STATE_ENDED — used to surface the "End of series"
    // banner when there's no next episode to chain into. Reset to
    // false on STATE_READY so a buffer-underrun retry doesn't keep
    // the banner stuck on.
    var playerEnded by remember(infohash, fileIdx) {
        mutableStateOf(false)
    }
    // Crossed the 95 % mark at least once this mount. Drives the native
    // "Next episode" control-bar button — a Netflix-style nudge that shows up
    // around the credit roll. Deliberate action only (no countdown).
    var nearEnd by remember(infohash, fileIdx) {
        mutableStateOf(false)
    }
    var grabbing by remember(infohash, fileIdx) { mutableStateOf(false) }
    LaunchedEffect(infohash, fileIdx) {
        val ctx = runCatching {
            container.apiFor(serverUrl).episodeContext(infohash, fileIdx)
        }.getOrNull()
        currentEpisode = ctx?.current
        nextEpisode = ctx?.next
    }

    // Tier-F fallback gate: flipped to `true` by the error listener
    // below when ExoPlayer chokes on the container / decoder / codec
    // (DV, Atmos JOC, exotic HEVC profiles…). One-shot per playback
    // session — `remember(infohash, fileIdx)` resets it when the user
    // navigates to a different file.
    var useRemuxFallback by remember(infohash, fileIdx) { mutableStateOf(false) }
    // Position captured at the moment the fallback fires, so the new
    // HLS player resumes where the user actually was rather than
    // restarting from 0. Defaults to 0 (= use the props-supplied
    // `startPositionSec`) when no fallback has happened yet.
    var fallbackResumeMs by remember(infohash, fileIdx) { mutableLongStateOf(0L) }

    // Per-stream AV1 decode routing, from the probed codec + bit depth.
    // 8-bit AV1 hardware-decodes on ANY AV1 silicon; 10-bit additionally
    // needs the decoder to declare Main10 (`hardwareAv1Main10`). Must stay
    // in lockstep with the `Iris-Caps` header and `buildPlayer`'s renderer
    // ordering — see `IrisCaps`.
    val probedAv1 = remember(probe) {
        probe.video.firstOrNull()?.takeIf { it.codec.equals("av1", ignoreCase = true) }
    }
    val av1HardwareFits = remember(probedAv1) {
        when {
            probedAv1 == null -> false
            (probedAv1.bitDepth ?: 8) >= 10 -> studio.kahn.iris.tv.data.IrisCaps.hardwareAv1Main10
            else -> studio.kahn.iris.tv.data.IrisCaps.hasHardwareDecoder("av1")
        }
    }

    // Proactive route to the server transcode. A 10-bit AV1 file on a box
    // whose silicon can't take it — none at all (Amlogic S905X2), or
    // 8-bit-only — stutters in dav1d software, so play the server's HEVC
    // re-encode from the start instead of direct-playing /stream and
    // bouncing on the stutter. Mirrors the server's `decide_video_mode`;
    // every other case direct-plays (8-bit AV1 without silicon stays on
    // dav1d, which sustains 8-bit fine).
    val needsServerTranscode = remember(probedAv1, av1HardwareFits) {
        probedAv1 != null && (probedAv1.bitDepth ?: 8) >= 10 && !av1HardwareFits
    }

    val playUrl =
        remember(serverUrl, infohash, fileIdx, useRemuxFallback, needsServerTranscode) {
            val base = if (serverUrl.endsWith("/")) serverUrl else "$serverUrl/"
            if (useRemuxFallback || needsServerTranscode) {
                // Server-side HLS remux/transcode. `play/master.m3u8` wraps the
                // source video in fragmented MP4 + audio in AAC (and re-encodes
                // the video to a codec the box decodes in hardware when the
                // client asked for it via `Iris-Caps`), sidestepping Media3's
                // MKV demuxer + the picky native codecs that refused /stream.
                "${base}api/torrents/$infohash/files/$fileIdx/play/master.m3u8"
            } else {
                // Default: raw source bytes — Media3 demuxes the
                // container in-process and surfaces every audio +
                // subtitle track (incl. PGS bitmap subs from Blu-rays).
                "${base}api/torrents/$infohash/files/$fileIdx/stream"
            }
        }

    // Flipped true on Media3's first rendered frame for the current
    // `playUrl`. Gates the transcode/remux loader overlay: keep it up
    // (showing `/status` progress) until the player ACTUALLY paints a frame,
    // not merely until the server reports the head is built. That covers a
    // resume into a not-yet-encoded position — the head is "ready" but the
    // player is still waiting on segments around the playhead — which
    // otherwise vanished into a silent black screen. Re-armed per `playUrl`
    // so the Tier-F swap (or a new episode) starts the loader fresh.
    var firstFrameRendered by remember(playUrl) { mutableStateOf(false) }

    // Resume position the player should start at (saved progress, or the
    // mid-stream Tier-F fallback position).
    val resumeMs = remember(playUrl, startPositionSec, fallbackResumeMs) {
        maxOf((startPositionSec * 1000).toLong(), fallbackResumeMs).coerceAtLeast(0)
    }

    // Authoritative film duration from ffprobe (the source). A still-growing
    // HLS EVENT transcode reports `player.duration` as only the encoder's edge,
    // so using it for saved progress / completion would record a position that
    // is a % of "what's transcoded so far", not of the whole film — wrong
    // resume points, premature "completed", a bogus continue-watching bar.
    // Prefer this everywhere; fall back to `player.duration` only when ffprobe
    // gave no duration (then they're equal anyway for the seekable-VOD paths).
    val filmDurationMs = remember(probe) {
        ((probe.durationSeconds ?: 0.0) * 1000).toLong()
    }

    // BOTH server paths stream a GROWING HLS EVENT playlist that only lists
    // segments up to the encoder's edge: the proactive transcode
    // (`needsServerTranscode`) AND — since the Tier-F remux now RE-ENCODES to
    // downscale >1080p sources (it used to stream-copy, fully seekable) — the
    // reactive `useRemuxFallback`. Preparing the player against such a stream
    // before the encoder has reached the playhead is the "stuck on loading"
    // bug: a resume seek past the edge waits for the encode to catch up, and a
    // cold `master.m3u8` blocks server-side until the head — long enough on a
    // CPU transcode to out-wait the player's IO timeout, exhaust its retries,
    // and leave it dead even after the encode finishes (the user had to back
    // out + re-enter). So we DON'T prepare yet: show a determinate bar and only
    // start once `/status` says the encoder passed the resume point (+10 s
    // lead) — i.e. against a warm cache, exactly what re-entering achieves.
    //
    // The proactive path keeps its resume-only gate (a fresh AV1 start direct
    // from the head was already fine); the reactive remux is ALWAYS gated,
    // because its cold-start is precisely what was failing.
    val gateOnServerBuild = useRemuxFallback || (needsServerTranscode && resumeMs > 0)
    var portionReady by remember(playUrl) { mutableStateOf(!gateOnServerBuild) }
    // Latest `/status` payload, polled here and rendered by the loader overlay.
    var serverStatus by remember(playUrl) {
        mutableStateOf<studio.kahn.iris.tv.data.PlayStatus?>(null)
    }
    if (needsServerTranscode || useRemuxFallback) {
        LaunchedEffect(playUrl) {
            val api = container.apiFor(serverUrl)
            val durationSec = probe.durationSeconds ?: 0.0
            val resumeSec = resumeMs / 1000.0
            // Poll until the player actually paints a frame (which hides the
            // loader). 1.5 s cadence matches the web client.
            while (!firstFrameRendered) {
                val st = runCatching { api.playStatus(infohash, fileIdx) }.getOrNull()
                if (st != null) {
                    serverStatus = st
                    if (gateOnServerBuild && !portionReady) {
                        // Encoded seconds = fraction × runtime. Start once the
                        // encoder is a touch past the resume point so the seek
                        // lands with buffer ahead instead of on the live edge.
                        // (resumeSec == 0 → fires after ~10 s encoded, i.e. a
                        // warm head, never a cold blocking request.)
                        val encodedSec = (st.progress ?: 0.0) * durationSec
                        if (st.ready || durationSec <= 0.0 || encodedSec >= resumeSec + 10.0) {
                            portionReady = true
                        }
                    }
                }
                delay(1_500L)
            }
        }
    }

    // Subtitles. On the raw `/stream` path the source MKV / MP4 already
    // carries every track and Media3 parses SRT / ASS / PGS in-process — no
    // injection needed. But the server HLS path (`useRemuxFallback` /
    // `needsServerTranscode`) ships a master playlist with NO subtitle
    // renditions (subs stay external), so Media3 would surface none. There we
    // side-load each text-based source sub as WebVTT from the dedicated route
    // (`/sub/{absoluteIndex}/track.vtt`). PGS bitmap subs can't become WebVTT
    // and are skipped on this path — rare, and the direct path still shows them.
    val sideLoadedSubs = remember(
        probe,
        serverUrl,
        infohash,
        fileIdx,
        useRemuxFallback,
        needsServerTranscode,
    ) {
        if (!useRemuxFallback && !needsServerTranscode) {
            emptyList()
        } else {
            val base = if (serverUrl.endsWith("/")) serverUrl else "$serverUrl/"
            probe.subtitle.filter { it.textBased }.map { s ->
                studio.kahn.iris.tv.data.webVttSubtitle(
                    url = "${base}api/torrents/$infohash/files/$fileIdx/sub/${s.absoluteIndex}/track.vtt",
                    language = s.language,
                    label = s.title ?: s.language,
                    forced = s.forced,
                )
            }
        }
    }

    val title by remember(torrent?.name, currentEpisode) {
        mutableStateOf(buildPlaybackTitle(torrent?.name, currentEpisode))
    }

    // `preferPlatformAv1` only matters on the direct-play path — the
    // transcode/remux URLs carry H.264/HEVC, which hardware-decodes under
    // either renderer order (dav1d only claims AV1).
    val player = remember(playUrl, av1HardwareFits) {
        buildPlayer(
            context,
            container.mediaOkHttpClient,
            preferPlatformAv1 = av1HardwareFits,
        )
    }
    // Start playback once the resume portion is ready (`portionReady`) — in one
    // shot, directly at the resume position. For non-transcode and fresh
    // (resume == 0) paths `portionReady` is already true, so this fires
    // immediately, preserving the prior "prepare on mount" behaviour.
    LaunchedEffect(player, portionReady) {
        if (!portionReady) return@LaunchedEffect
        player.setMediaItem(buildMediaItem(playUrl, title, subtitles = sideLoadedSubs), resumeMs)
        player.prepare()
        player.playWhenReady = true
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
    LaunchedEffect(player, probe, initialAudioIdx, initialSubIdx, prefAudioLang, prefSubLang) {
        val savedAudioLang = savedAudioOrdinal?.let { probe.audio[it].language }
        // Audio precedence: this file's saved track, else the per-user
        // language preference (carries across episodes), else the source
        // default. A missing language just lets Media3 fall back.
        val initialAudio = savedAudioLang
            ?: prefAudioLang
            ?: probe.audio.firstOrNull { it.default }?.language
            ?: probe.audio.firstOrNull()?.language
        val savedSubLang = savedSubOrdinal
            ?.takeIf { it >= 0 && it in probe.subtitle.indices }
            ?.let { probe.subtitle[it].language }
        val params = player.trackSelectionParameters.buildUpon()
        if (initialAudio != null) params.setPreferredAudioLanguage(initialAudio)
        when {
            // This file's saved subtitle pick (a specific language).
            savedSubLang != null -> {
                params.setPreferredTextLanguage(savedSubLang)
                params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
            }
            // This file's explicit "off".
            savedSubOrdinal == -1 -> params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
            // No per-file pick → per-user subtitle preference.
            prefSubLang == "off" -> params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
            // Enable only when the preferred language is actually present —
            // never force a different language onto the user.
            prefSubLang != null &&
                probe.subtitle.any { it.language.equals(prefSubLang, ignoreCase = true) } -> {
                params.setPreferredTextLanguage(prefSubLang)
                params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
            }
            // No preference (or preferred language absent): subs off (matches
            // the previous default — the gear menu still lets the user enable).
            else -> params.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
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
                    player.currentPosition to
                        (filmDurationMs.takeIf { it > 0 } ?: player.duration.takeIf { it > 0 })
                }
                runCatching {
                    container.apiFor(serverUrl).saveProgress(
                        infohash = infohash,
                        idx = fileIdx,
                        body = ProgressUpdate(
                            positionSeconds = pos / 1000.0,
                            durationSeconds = dur?.div(1000.0),
                            audioTrackIdx = currentAudioIdxRef.get()?.toLong(),
                            subtitleTrackIdx = currentSubIdxRef.get()?.toLong(),
                            completed = isWatched(pos, dur),
                        ),
                    )
                }
                // Also remember the chosen LANGUAGES per-user so they carry to
                // the next episode / device. Resolved from the current track
                // selection; "off" when subtitles are disabled.
                val audioLang = currentAudioIdxRef.get()
                    ?.let { idx -> probe.audio.firstOrNull { it.index == idx }?.language }
                val subLang = when (val s = currentSubIdxRef.get()) {
                    null -> null
                    -1 -> "off"
                    else -> probe.subtitle.firstOrNull { it.index == s }?.language
                }
                runCatching {
                    container.apiFor(serverUrl).savePlaybackPreferences(
                        UpdatePlaybackPrefs(audioLanguage = audioLang, subtitleLanguage = subLang),
                    )
                }
            }
            pendingTrackSaveJob.set(job)
        }
    }

    // Seek hint posting. Mirror of the web client's `postSeekHint`
    // (`web/src/lib/iris-core/manifest-client.ts`). On every user-
    // initiated seek we fire a fire-and-forget POST so the server can
    // bias librqbit's piece priority toward ~30 s of bytes forward of
    // the new playhead. Without this, a rewind to a piece librqbit
    // hasn't kept hot (e.g. user resumed mid-file then scrolled back)
    // makes Media3's read block on slow piece delivery — the freeze
    // looks like "buffering forever" because the server is genuinely
    // waiting on bytes.
    //
    // Byte offset is a linear approximation from playhead × file_size /
    // duration; the server's `prefetch_range` widens the priority bias
    // around it, so an off-by-a-few-MB estimate is fine.
    val fileSizeBytes: Long = remember(torrent, fileIdx) {
        torrent?.files?.firstOrNull { it.index == fileIdx }?.sizeBytes ?: 0L
    }
    // Latched on every user seek, consumed by the next progress save. The
    // server's reset guard refuses a near-zero position over substantial
    // stored progress unless the save carries `seek = true` (mirror of the
    // web client's `seekPendingRef`).
    val pendingSeekSave = remember { java.util.concurrent.atomic.AtomicBoolean(false) }
    DisposableEffect(player, fileSizeBytes) {
        val listener = object : androidx.media3.common.Player.Listener {
            override fun onPositionDiscontinuity(
                oldPosition: androidx.media3.common.Player.PositionInfo,
                newPosition: androidx.media3.common.Player.PositionInfo,
                reason: Int,
            ) {
                if (reason != androidx.media3.common.Player.DISCONTINUITY_REASON_SEEK) return
                pendingSeekSave.set(true)
                val durMs = filmDurationMs.takeIf { it > 0 } ?: player.duration
                if (durMs <= 0 || fileSizeBytes <= 0) return
                val playheadS = newPosition.positionMs / 1000.0
                val byteOffset = (
                    (newPosition.positionMs.toDouble() / durMs.toDouble()) * fileSizeBytes
                ).toLong().coerceIn(0L, fileSizeBytes - 1)
                container.applicationScope.launch {
                    runCatching {
                        container.apiFor(serverUrl).postSeekHint(
                            infohash = infohash,
                            idx = fileIdx,
                            body = studio.kahn.iris.tv.data.SeekHint(
                                byteOffset = byteOffset,
                                playheadS = playheadS,
                            ),
                        )
                    }
                }
            }
        }
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
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
                // Tier-F fallback: codec / container / decoder
                // failures usually mean Media3 + the device's
                // native decoders can't chew through this exact
                // mix of HEVC / DV / Atmos / MKV. The server's HLS
                // remux pipeline (`/play/master.m3u8`) rewraps the
                // stream into fragmented MP4 with AAC audio,
                // sidestepping every one of those choke points.
                // One shot per session — if THAT also fails we
                // surface the error properly instead of looping.
                if (!useRemuxFallback && studio.kahn.iris.tv.data.isRemuxableError(error)) {
                    android.util.Log.i(
                        "iris-core",
                        "switching to server-side HLS remux after ${error.errorCodeName}",
                    )
                    fallbackResumeMs = player.currentPosition.coerceAtLeast(0L)
                    useRemuxFallback = true
                    return
                }
                onPlayerError(message)
            }

            override fun onRenderedFirstFrame() {
                // The player painted a real frame — playback has truly
                // started, so dismiss the transcode/remux loader overlay
                // (gated on this below). Fires once per prepared stream,
                // after the resume seek lands, so it's the correct "we're
                // actually showing video now" signal.
                firstFrameRendered = true
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

                // Direct-play audio completeness telemetry. Silent loss
                // modes: MatroskaExtractor DROPS tracks whose CodecID it
                // doesn't recognise (group missing entirely), and the
                // native gear menu HIDES tracks below FORMAT_HANDLED or
                // flagged forced (the latter is neutralised for audio by
                // `ForcedAudioVisibleExtractorsFactory`). Any hit here is a
                // genuine extractor drop or decoder gap worth a bug
                // report — log it; video + remaining audio play fine.
                if (
                    !useRemuxFallback && !needsServerTranscode &&
                    tracks.groups.isNotEmpty() &&
                    (
                        audioGroups.size < probe.audio.size ||
                            audioGroups.any { !it.isSupported }
                        )
                ) {
                    android.util.Log.w(
                        "iris-core",
                        "direct-play audio deficit: ${audioGroups.size} surfaced " +
                            "(${audioGroups.count { it.isSupported }} supported) " +
                            "of ${probe.audio.size} probed",
                    )
                }

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
        // Real film duration (ffprobe) — NOT the growing transcode's edge.
        var durationMs: Long = filmDurationMs.takeIf { it > 0 } ?: -1
        val tick = object : Runnable {
            override fun run() {
                // Only fall back to the player's (live-window) duration when
                // ffprobe gave us nothing.
                if (durationMs <= 0 && player.duration > 0) durationMs = player.duration
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
                        val completed = isWatched(pos, durationMs.takeIf { it > 0 })
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
                                        audioTrackIdx = audioIdx?.toLong(),
                                        subtitleTrackIdx = subIdx?.toLong(),
                                        completed = completed,
                                        seek = pendingSeekSave.getAndSet(false),
                                        playing = true,
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
            val dur = filmDurationMs.takeIf { it > 0 } ?: player.duration.takeIf { it > 0 }
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
                            audioTrackIdx = audioIdx?.toLong(),
                            subtitleTrackIdx = subIdx?.toLong(),
                            completed = isWatched(pos, dur),
                            seek = pendingSeekSave.getAndSet(false),
                        ),
                    )
                }
            }
            player.release()
        }
    }

    // Pause + cut the stream when the user leaves via Home (`ON_STOP`).
    // Without this the torrent stream and the tick loop above kept running
    // in the background, draining bandwidth indefinitely. `pause()` stops
    // ExoPlayer from pulling further byte ranges — `WAKE_MODE_NETWORK`
    // (see `PlayerFactory.buildPlayer`) releases its wake lock automatically
    // once paused — and we push one immediate progress save (instead of
    // waiting for the next 7s tick) so the admin presence view reflects the
    // paused state right away. No auto-resume on `ON_START`: the user
    // presses Play explicitly, matching the Netflix/YouTube-TV convention.
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    DisposableEffect(player, lifecycleOwner) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_STOP) {
                player.pause()
                val pos = player.currentPosition
                if (pos > 0) {
                    val dur = filmDurationMs.takeIf { it > 0 } ?: player.duration.takeIf { it > 0 }
                    val audioIdx = currentAudioIdxRef.get()
                    val subIdx = currentSubIdxRef.get()
                    container.applicationScope.launch {
                        runCatching {
                            container.apiFor(serverUrl).saveProgress(
                                infohash = infohash,
                                idx = fileIdx,
                                body = ProgressUpdate(
                                    positionSeconds = pos / 1000.0,
                                    durationSeconds = dur?.div(1000.0),
                                    audioTrackIdx = audioIdx?.toLong(),
                                    subtitleTrackIdx = subIdx?.toLong(),
                                    completed = isWatched(pos, dur),
                                    playing = false,
                                ),
                            )
                        }
                    }
                }
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
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
    // `iris_next_episode` lives in the controller layout (not a Compose
    // overlay) so the D-pad reaches it like every other transport control —
    // see the LaunchedEffect below for the visibility/click wiring.
    var nextEpisodeButton by remember { mutableStateOf<android.widget.ImageButton?>(null) }

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
            IrisPlayerView(ctx).apply {
                this.player = player
                setShowSubtitleButton(true)
                setShowFastForwardButton(true)
                setShowRewindButton(true)
                layoutParams = android.widget.FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT)
                // Hold the screen-on flag for as long as the PlayerView is
                // attached. Without this, Android TV blanks the panel after
                // its idle timeout (typically 2-5 min) since the remote
                // sees no key events during continuous playback. The flag
                // is dropped automatically when the view is detached.
                keepScreenOn = true
                titleView = findViewById(R.id.iris_title)
                // Back-to-Home lives in the controller layout so it's D-pad
                // reachable like the transport controls (the PlayerView keeps
                // focus); wire its click to the nav-up callback.
                findViewById<android.widget.ImageButton>(R.id.iris_back_home)
                    ?.setOnClickListener { onBack() }
                nextEpisodeButton = findViewById(R.id.iris_next_episode)
                installIrisTrackNameProvider(this)
                playerView = this
            }
        },
        update = { it.player = player },
    )

    // Next-episode button: visible once we're past 95% of the runtime (or
    // the player has ended) and there's somewhere to go. A single tap plays
    // it immediately if already downloaded, otherwise grabs it first (the
    // prepare-next auto-continuation usually means it's already downloaded
    // by the time playback gets here). Lives in the native controller layout
    // — see `nextEpisodeButton` above — so the D-pad reaches it like any
    // other transport control instead of being stranded in a Compose overlay
    // outside PlayerView's focus hierarchy.
    LaunchedEffect(nextEpisodeButton, nearEnd, playerEnded, isMovie, nextEpisode, grabbing) {
        val btn = nextEpisodeButton ?: return@LaunchedEffect
        val next = nextEpisode
        val visible = (nearEnd || playerEnded) && !isMovie && next != null
        btn.visibility = if (visible) android.view.View.VISIBLE else android.view.View.GONE
        if (!visible) return@LaunchedEffect
        btn.isEnabled = !(grabbing && next.status == EpisodeStatus.available)
        btn.setOnClickListener {
            val ih = next.infohash
            val idx = next.fileIdx
            if (next.status == EpisodeStatus.downloaded && ih != null && idx != null) {
                onNavigateToFile(ih, idx.toInt())
                return@setOnClickListener
            }
            val fid = next.followId ?: return@setOnClickListener
            grabbing = true
            scope.launch {
                val grabbed = runCatching {
                    container.apiFor(serverUrl).grabEpisode(
                        id = fid.toString(),
                        season = next.season.toInt(),
                        episode = next.episode.toInt(),
                    )
                }.getOrNull()
                grabbing = false
                if (grabbed != null) onNavigateToFile(grabbed.infohash, grabbed.fileIdx.toInt())
            }
        }
    }

    // Episode navigation is the native control-bar "Next episode" button
    // (`nextEpisodeButton`, wired above) — D-pad-reachable inside PlayerView's
    // focus hierarchy. The old top-right "‹ Prev / Next ›" Compose chips lived
    // OUTSIDE that hierarchy, so the controller always held focus and they
    // could never be reached; removed. Previous-episode selection lives on the
    // series screen. This Box still hosts the remux + end-of-playback overlays.
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 32.dp, vertical = 24.dp),
    ) {
        // Remux/transcode overlay: poll `/play/status` and surface the
        // server's progress while it builds the HLS variant. Shown when the
        // error listener swapped us onto the remux (`useRemuxFallback`) OR when
        // we proactively routed a heavy 10-bit AV1 to the server transcode
        // (`needsServerTranscode`). Stays up until the player paints its first
        // frame — which, for a resume into a transcode, is only after the
        // encode has passed the resume point and playback has started.
        if ((useRemuxFallback || needsServerTranscode) && !firstFrameRendered) {
            Box(modifier = Modifier.align(Alignment.Center)) {
                RemuxFallbackOverlay(status = serverStatus)
            }
        }

        // Movies have no episode taxonomy, so `currentEpisode` is null — show a
        // movie-worded end pill instead of the series-finale copy. A followed
        // series with no next keeps its own finale copy. The "play the next
        // episode" action itself now lives in the native control bar (see
        // `nextEpisodeButton` above), not here.
        if (playerEnded && (isMovie || nextEpisode == null)) {
            Box(
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(bottom = 96.dp),
            ) {
                EndOfPlaybackPill(isMovie = isMovie)
            }
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun EndOfPlaybackPill(isMovie: Boolean) {
    // Informational only — the reachable "Back to Home" action is the button in
    // the player controller (exo_player_control_view.xml). A button here would
    // sit in a Compose overlay the D-pad can't reach while the PlayerView holds
    // focus, so it's intentionally absent.
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
            Eyebrow(if (isMovie) "You're all set" else "You're all caught up")
            Text(
                if (isMovie) "You've finished watching." else "No more episodes available.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun LoadingOverlay(
    error: String?,
    gone: Boolean,
    regrabbing: Boolean,
    probeReady: Boolean,
    torrent: TorrentView?,
    onRetry: () -> Unit,
    onRegrab: () -> Unit,
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
                if (gone) {
                    Text(
                        text = "It was probably reclaimed to free up space. " +
                            "Grab it again and playback picks up right where it left off.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Row(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    modifier = Modifier.padding(top = 12.dp),
                ) {
                    if (gone) {
                        IrisButton(if (regrabbing) "Grabbing…" else "Grab it again", onRegrab)
                    } else {
                        IrisButton("Retry", onRetry)
                    }
                    IrisButton("Back", onBack, variant = IrisButtonVariant.Ghost)
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
        val label = if (torrent.state == TorrentState.paused) {
            "Download paused"
        } else if (torrent.error != null) {
            "Torrent error"
        } else if (torrent.downloadSpeedBps > 0 || torrent.peers > 0) {
            "Downloading…"
        } else {
            "Connecting to peers…"
        }
        return Step(label, sub, pct.toFloat().coerceIn(0f, 0.99f))
    }
    return Step("Reading media metadata…", "ffprobe scanning streams.", null)
}

/**
 * Loader overlay shown while the server builds the HLS variant (Tier-F
 * remux, or the proactive AV1-10-bit transcode). The caller owns the
 * `/play/status` polling and visibility — it keeps this mounted until the
 * player paints its first frame, and on a transcode resume that's only
 * after the encode has reached the resume point. This composable just
 * renders the latest `status` (`reason` / `progress`).
 */
@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
private fun RemuxFallbackOverlay(
    status: studio.kahn.iris.tv.data.PlayStatus?,
) {
    val s = status
    Surface(
        shape = RoundedCornerShape(16.dp),
        colors = SurfaceDefaults.colors(
            containerColor = Color.Black.copy(alpha = 0.85f),
            contentColor = Color.White,
        ),
    ) {
        Column(
            Modifier.padding(horizontal = 32.dp, vertical = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                "Switching to remuxed stream",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                color = Color.White,
            )
            val (label, pct) = when (s?.reason) {
                "downloading" -> "Downloading source" to s.progress
                "remuxing" -> "Remuxing on the server" to s.progress
                else -> "Preparing" to null
            }
            Text(
                label + (pct?.let { " · ${(it * 100).toInt()}%" } ?: "…"),
                style = MaterialTheme.typography.bodyMedium,
                color = Color.White.copy(alpha = 0.85f),
            )
            // Determinate progress bar when the server is feeding us
            // a fraction; otherwise an indeterminate sliver so the
            // overlay never freezes silent on a slow remuxer warm-up.
            if (pct != null) {
                androidx.compose.material3.LinearProgressIndicator(
                    progress = { pct.toFloat().coerceIn(0f, 1f) },
                    modifier = Modifier.fillMaxWidth(0.5f).height(4.dp),
                    color = MaterialTheme.colorScheme.primary,
                    trackColor = Color.White.copy(alpha = 0.15f),
                )
            } else {
                androidx.compose.material3.LinearProgressIndicator(
                    modifier = Modifier.fillMaxWidth(0.5f).height(4.dp),
                    color = MaterialTheme.colorScheme.primary,
                    trackColor = Color.White.copy(alpha = 0.15f),
                )
            }
            s?.error?.let { err ->
                Text(
                    err,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }
        }
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
