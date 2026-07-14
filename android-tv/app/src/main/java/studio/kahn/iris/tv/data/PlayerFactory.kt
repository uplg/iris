package studio.kahn.iris.tv.data

import android.content.Context
import android.os.Handler
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.Renderer
import androidx.media3.exoplayer.mediacodec.MediaCodecSelector
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.video.VideoRendererEventListener
import okhttp3.OkHttpClient
import studio.kahn.iris.tv.BuildConfig

/**
 * Build an `ExoPlayer` whose HTTP layer reuses our session cookies via a
 * dedicated, Media3-tuned OkHttp client. The caller MUST pass the
 * `mediaOkHttpClient` from `AppContainer` — never the bare `okHttpClient`
 * shared with Retrofit. See `deriveMediaOkHttpClient` in `HttpClient.kt`
 * for the Fire-TV-specific reason that isolation matters (shared pool +
 * HTTP/2 stream cancel = wedged TCP connection on Fire OS).
 */
@UnstableApi
fun buildPlayer(
    context: Context,
    mediaOkHttp: OkHttpClient,
    userAgent: String = "iris-tv/${BuildConfig.VERSION_NAME} (Media3)",
    /** Rank software MediaCodec decoders above hardware ones. Live-TV rescue
     *  mode: some IPTV restreams are interlaced + reference-frame-corrupt
     *  H.264 (M6 notably) that a box's HARDWARE decoder swallows without ever
     *  producing a frame — an eternal silent BUFFERING with no error — while
     *  software decoders power through like browsers do. Costs CPU, so only
     *  the live player flips it, and only after a hardware attempt stalled. */
    preferSoftwareVideo: Boolean = false,
    /** Rank the PLATFORM video decoders above the bundled dav1d extension
     *  for this playback. Set by `WatchScreen` when the probed stream is
     *  AV1 the device's silicon can genuinely decode (8-bit on any AV1
     *  hardware, 10-bit only when `IrisCaps.hardwareAv1Main10`) — a weak
     *  CPU box like the Chromecast HD (S805X2, 4×A35) drops frames
     *  dav1d-decoding 1080p AV1 its hardware chews through. Defaults to
     *  false: non-AV1 formats are hardware-decoded either way (dav1d only
     *  claims AV1), so callers without a probe (live TV) lose nothing. */
    preferPlatformAv1: Boolean = false,
): ExoPlayer {
    val dataSourceFactory = OkHttpDataSource.Factory(mediaOkHttp).setUserAgent(userAgent)
    val mediaSourceFactory = DefaultMediaSourceFactory(context)
        .setDataSourceFactory(dataSourceFactory)

    // Renderers factory. The FFmpeg decoder extension is built and
    // dropped into `app/libs/` by `scripts/build-ffmpeg-ext.sh`; when
    // absent (fresh clone), `DefaultRenderersFactory` silently skips
    // the missing renderer class via reflection — playback works for
    // anything Android handles natively but DTS / DTS-HD MA / TrueHD
    // / MLP go silent.
    //
    // `EXTENSION_RENDERER_MODE_PREFER` puts the bundled software
    // extensions BEFORE the platform `MediaCodec*Renderer`s — for both
    // the FFmpeg audio decoder AND the dav1d AV1 video decoder (built by
    // `scripts/build-{ffmpeg,av1}-ext.sh`). We need PREFER, not the
    // lighter `ON`, for two distinct reasons:
    //
    //   - Audio (always PREFER): some Android TV builds (AVD emulator,
    //     some AFTV firmwares) register a MediaCodec that claims DTS
    //     support but outputs silence; with ON, ExoPlayer trusts the
    //     claim and never falls through to FFmpeg. PREFER lets FFmpeg
    //     handle everything it supports; the platform renderer only sees
    //     the rest.
    //
    //   - Video (PREFER unless `preferPlatformAv1`): some TV boxes have
    //     an AV1 hardware decoder that is 8-bit-only (their HEVC path
    //     does 10-bit, AV1 does not). With ON the platform renderer is
    //     tried first for a 10-bit AV1 stream, reports support, then
    //     fails at runtime with DECODING_FORMAT_EXCEEDS_CAPABILITIES /
    //     DECODER_INIT_FAILED — and `isRemuxableError` treats that as a
    //     cue to bounce onto the server HLS remux (`/play/master.m3u8`)
    //     instead of decoding. PREFER routes those streams straight to
    //     dav1d (handles 8- and 10-bit). But a blanket PREFER starves
    //     capable silicon too: a weak-CPU box with a real AV1 decoder
    //     (Chromecast HD, S805X2: 4×A35) was dav1d-decoding 1080p AV1
    //     its hardware chews through, and dropped frames in high-motion
    //     scenes. So the VIDEO renderer order is per-PLAYBACK (see the
    //     `buildVideoRenderers` override below): when the caller probed
    //     the stream and vouched for the silicon (`preferPlatformAv1`) →
    //     ON (hardware first, dav1d as capability fallback), otherwise →
    //     PREFER (dav1d first). dav1d only ever claims AV1, so HEVC /
    //     H.264 / VP9 hardware-decode via the platform either way.
    //
    // NOTE: the AV1 AAR MUST contain native `libdav1dJNI.so` — a
    // classes-only AAR (the dav1d build was skipped) leaves the renderer
    // inert and AV1 silently falls back to the slow platform decoder.
    // `build-av1-ext.sh` guards against shipping a hollow AAR.
    //
    // `setEnableDecoderFallback(true)` is belt-and-braces: if the
    // selected renderer hits a runtime init failure (corrupted .so,
    // missing symbol on an exotic ABI, …), ExoPlayer transparently
    // retries with the next renderer instead of bubbling an error to
    // the user.
    val renderersFactory = object : DefaultRenderersFactory(context) {
        override fun buildVideoRenderers(
            context: Context,
            extensionRendererMode: Int,
            mediaCodecSelector: MediaCodecSelector,
            enableDecoderFallback: Boolean,
            eventHandler: Handler,
            eventListener: VideoRendererEventListener,
            allowedVideoJoiningTimeMs: Long,
            out: ArrayList<Renderer>,
        ) {
            val videoMode = if (preferPlatformAv1) {
                EXTENSION_RENDERER_MODE_ON
            } else {
                extensionRendererMode
            }
            super.buildVideoRenderers(
                context,
                videoMode,
                mediaCodecSelector,
                enableDecoderFallback,
                eventHandler,
                eventListener,
                allowedVideoJoiningTimeMs,
                out,
            )
        }
    }
        .setExtensionRendererMode(DefaultRenderersFactory.EXTENSION_RENDERER_MODE_PREFER)
        .setEnableDecoderFallback(true)
        .apply {
            // See the `preferSoftwareVideo` doc above. PREFER_SOFTWARE ranks
            // software codecs first for every MediaCodec renderer; audio is
            // unaffected in practice (the ffmpeg extension already outranks
            // MediaCodec audio via EXTENSION_RENDERER_MODE_PREFER).
            if (preferSoftwareVideo) {
                setMediaCodecSelector(
                    androidx.media3.exoplayer.mediacodec.MediaCodecSelector.PREFER_SOFTWARE,
                )
            }
        }

    return ExoPlayer.Builder(context)
        .setRenderersFactory(renderersFactory)
        .setMediaSourceFactory(mediaSourceFactory)
        .setSeekBackIncrementMs(10_000)
        .setSeekForwardIncrementMs(30_000)
        .build()
        .apply {
            // Without a wake mode the CPU goes to sleep on idle TV
            // hardware (no remote events for >2 min during a quiet
            // dialogue scene) and playback stalls. NETWORK keeps a
            // partial wake lock + WiFi lock while playing so the
            // byte-range fetches keep flowing. The lock is released
            // automatically when playback pauses or `release()` is
            // called.
            setWakeMode(C.WAKE_MODE_NETWORK)
            // Audio attributes intentionally left at Media3 defaults.
            // We previously injected `USAGE_MEDIA` +
            // `CONTENT_TYPE_MOVIE` + `handleAudioFocus=true` to
            // nudge `DefaultAudioSink` toward passthrough, but
            // re-configuring those across a runtime audio-track
            // switch (AAC → AC-3 or 5.1 → stereo) destabilised the
            // sink on some Android TV builds — visible as a hard
            // playback crash on track-change. Defaults already
            // discover surround capabilities via `AudioCapabilities`
            // and route passthrough correctly.
        }
}

/**
 * Translates a Media3 [PlaybackException] into a (user-readable
 * message, isTransient) pair. The boolean drives the auto-retry
 * heuristic in `WatchScreen` — transient errors (network blip,
 * mid-stream decoder hiccup) are worth re-`prepare()`ing once or
 * twice; the rest need user attention.
 */
fun humanizePlaybackError(e: PlaybackException): Pair<String, Boolean> {
    val message = when (e.errorCode) {
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED,
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT,
        -> "Connection to the Iris server was lost. Retrying…"
        PlaybackException.ERROR_CODE_IO_NO_PERMISSION,
        PlaybackException.ERROR_CODE_IO_INVALID_HTTP_CONTENT_TYPE,
        -> "The server refused this stream. You may need to sign in again."
        PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND -> "Source file not found."
        PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS -> "Server returned an error response."
        PlaybackException.ERROR_CODE_IO_UNSPECIFIED -> "Network read failed."
        PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED,
        PlaybackException.ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED,
        -> "This file's container format isn't supported by Media3 on this device."
        PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED,
        PlaybackException.ERROR_CODE_PARSING_MANIFEST_MALFORMED,
        -> "This file appears corrupted (bad container metadata)."
        PlaybackException.ERROR_CODE_DECODER_INIT_FAILED,
        PlaybackException.ERROR_CODE_DECODER_QUERY_FAILED,
        -> "This device doesn't have a decoder for this video / audio codec."
        PlaybackException.ERROR_CODE_DECODING_FAILED,
        PlaybackException.ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES,
        -> "Playback stalled: the decoder couldn't keep up."
        PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_WRITE_FAILED,
        -> "Audio output failed. Check your HDMI / speaker connection."
        else -> "Playback error (${e.errorCodeName})."
    }
    // Conservative: only re-prepare on genuinely network-shaped
    // errors. We used to also auto-retry on `DECODING_FAILED` and
    // `AUDIO_TRACK_WRITE_FAILED`, but those can fire mid-stream
    // during an audio-track switch (Media3 disabling + re-enabling
    // the audio renderer with a different codec) — re-`prepare()`
    // there kills playback instead of healing it. Decoder / audio
    // sink hiccups surface to the user immediately so they see what
    // actually broke.
    val transient = when (e.errorCode) {
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED,
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT,
        PlaybackException.ERROR_CODE_IO_UNSPECIFIED,
        -> true
        else -> false
    }
    return message to transient
}

/**
 * `true` when an `ExoPlayer` error is the kind a server-side HLS
 * remux could plausibly fix. Covers every container / decoder /
 * codec-capability failure: Media3's MKV demuxer choking on a
 * malformed segment, the native HEVC decoder rejecting a
 * Main10+DV profile combination, an Atmos JOC audio track the
 * platform can't pass through, etc. The remux pipeline rewraps to
 * fragmented MP4 and re-encodes the audio to AAC, which sidesteps
 * every one of those choke points.
 *
 * Deliberately excludes network / I/O errors — those would fail
 * the remux endpoint just as hard, and excludes the `AUDIO_TRACK_*`
 * codes which are platform output-sink issues unrelated to the
 * source format.
 */
fun isRemuxableError(e: PlaybackException): Boolean = when (e.errorCode) {
    PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED,
    PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED,
    PlaybackException.ERROR_CODE_DECODER_INIT_FAILED,
    PlaybackException.ERROR_CODE_DECODER_QUERY_FAILED,
    PlaybackException.ERROR_CODE_DECODING_FAILED,
    PlaybackException.ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES,
    -> true
    else -> false
}

/**
 * Build a `MediaItem` for the Iris `/stream` endpoint — the raw,
 * range-supported source bytes (MKV / MP4 / etc.) served as-is by
 * `iris-api`'s `stream_file` route.
 *
 * We deliberately bypass the server-side HLS-CMAF remux pipeline that
 * the web client uses as its Tier F fallback. Media3 / ExoPlayer
 * already does everything that pipeline does, but in-process:
 *   - MatroskaExtractor demuxes the container.
 *   - HEVC / H.264 video is hardware-decoded (or software-decoded via
 *     C2 on the SoC).
 *   - AC-3 / E-AC-3 / DTS audio is passed through to the HDMI sink
 *     when the receiver advertises support; otherwise Media3 falls
 *     back to its software AC-3 decoder.
 *   - Multi-audio + embedded subtitle tracks (SRT, ASS, PGS) are
 *     surfaced via the standard `Tracks` API and the native
 *     PlayerView settings menu.
 *
 * Savings vs the HLS pipeline: zero ffmpeg+shaka CPU on the server,
 * no remux wait at start, sub-stream handling is consistent with
 * the source file (PGS bitmap subs from Blu-rays render correctly
 * instead of being silently dropped at the VTT filter).
 *
 * Mime hint: `APPLICATION_MATROSKA` is the common case for our
 * library (every torrent we've ingested is `.mkv`). Caller can pass
 * a different mime via `mimeType` if probing surfaces something else.
 * Letting Media3 sniff also works (omit the hint) but pre-declaring
 * skips the first range probe.
 *
 * Resume position is intentionally NOT applied here via
 * `ClippingConfiguration` — that would re-window the timeline so
 * `player.duration` reports `(end - resume)` instead of the full file
 * duration (a 23-min episode came back as a 6-min one after resuming).
 * The caller seeks via `Player.setMediaItem(item, startPositionMs)`
 * which positions the playhead without touching the timeline.
 */
/**
 * Side-loaded WebVTT subtitle track for the server HLS path. The remux /
 * transcode master playlist carries NO subtitle renditions (subs stay
 * external, served by `/sub/{stream_idx}/track.vtt`), and Media3 only reads
 * embedded subs off the raw `/stream` container — so without side-loading,
 * the remux path shows no subtitles at all. `streamIdx` is the source's
 * ABSOLUTE stream index (`SubtitleStream.absoluteIndex`), matching the route.
 */
@UnstableApi
fun webVttSubtitle(
    url: String,
    language: String?,
    label: String?,
    forced: Boolean,
): MediaItem.SubtitleConfiguration =
    MediaItem.SubtitleConfiguration.Builder(android.net.Uri.parse(url))
        .setMimeType(MimeTypes.TEXT_VTT)
        .setLanguage(language)
        .setLabel(label ?: language ?: "Subtitles")
        .setSelectionFlags(if (forced) C.SELECTION_FLAG_FORCED else 0)
        .build()

@UnstableApi
fun buildMediaItem(
    playUrl: String,
    title: String? = null,
    mimeType: String = MimeTypes.APPLICATION_MATROSKA,
    subtitles: List<MediaItem.SubtitleConfiguration> = emptyList(),
): MediaItem {
    // CRITICAL: the server HLS routes (`/play/master.m3u8`, used by BOTH the
    // proactive AV1-10-bit transcode path AND the Tier-F error fallback) MUST
    // be tagged as HLS. When an explicit MIME is set, Media3's
    // `DefaultMediaSourceFactory` trusts it and builds a `ProgressiveMediaSource`
    // + `MatroskaExtractor` — which then tries to demux the `.m3u8` TEXT
    // playlist as a Matroska container and NEVER starts playback (the
    // "ready on the server but the TV never launches" bug). The default hint
    // is MKV for the raw `/stream` bytes, so without this an HLS `playUrl`
    // inherits the wrong type. Infer HLS from the URL so every caller routes
    // to `HlsMediaSource` automatically; the raw-stream path keeps its hint.
    val resolvedMime = if (isHlsUrl(playUrl)) MimeTypes.APPLICATION_M3U8 else mimeType
    return MediaItem.Builder()
        .setUri(playUrl)
        .setMimeType(resolvedMime)
        .apply {
            // Attach side-loaded subs ONLY when we have some (the server HLS
            // path). An empty list is a no-op AND `subtitleConfigurations`
            // already defaults to empty, but skipping the call entirely keeps
            // the raw `/stream` MediaItem byte-identical to before — zero risk
            // to Media3's native container-subtitle detection (the extractor
            // surfaces embedded SRT/ASS/PGS independently of this list; the
            // side-loaded tracks are merged ADDITIVELY via MergingMediaSource).
            if (subtitles.isNotEmpty()) {
                setSubtitleConfigurations(subtitles)
            }
            if (!title.isNullOrBlank()) {
                setMediaMetadata(MediaMetadata.Builder().setTitle(title).build())
            }
        }
        .build()
}

/** True when [url]'s path ends in `.m3u8` (HLS playlist), ignoring any
 *  query string / fragment. Drives the HLS-vs-progressive source-type
 *  selection in [buildMediaItem]. */
private fun isHlsUrl(url: String): Boolean =
    url.substringBefore('?').substringBefore('#').endsWith(".m3u8", ignoreCase = true)
