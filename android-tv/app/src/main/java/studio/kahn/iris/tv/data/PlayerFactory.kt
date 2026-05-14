package studio.kahn.iris.tv.data

import android.content.Context
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import okhttp3.OkHttpClient
import studio.kahn.iris.tv.BuildConfig

/**
 * Build an `ExoPlayer` whose HTTP layer reuses our OkHttp client (cookie
 * jar, timeouts, logging). Without this Media3 spins up its own
 * `DefaultHttpDataSource`, which does NOT see our session cookies, so the
 * HLS manifest + segment requests would 401 immediately.
 */
@UnstableApi
fun buildPlayer(
    context: Context,
    okHttp: OkHttpClient,
    userAgent: String = "iris-tv/${BuildConfig.VERSION_NAME} (Media3)",
): ExoPlayer {
    val dataSourceFactory = OkHttpDataSource.Factory(okHttp).setUserAgent(userAgent)
    val mediaSourceFactory = DefaultMediaSourceFactory(context)
        .setDataSourceFactory(dataSourceFactory)

    // Renderers factory. The FFmpeg decoder extension is built and
    // dropped into `app/libs/` by `scripts/build-ffmpeg-ext.sh`; when
    // absent (fresh clone), `DefaultRenderersFactory` silently skips
    // the missing renderer class via reflection — playback works for
    // anything Android handles natively but DTS / DTS-HD MA / TrueHD
    // / MLP go silent.
    //
    // `EXTENSION_RENDERER_MODE_PREFER` puts the FFmpeg renderer
    // BEFORE the platform `MediaCodecAudioRenderer`. We picked PREFER
    // over the lighter-touch `ON` because some Android TV builds
    // (notably the AVD emulator and some AFTV firmwares) register a
    // MediaCodec that claims DTS support but produces silence at
    // runtime — with mode ON, ExoPlayer trusts that claim and never
    // falls through to FFmpeg, so playback is silent. With PREFER,
    // FFmpeg handles every codec it supports; the platform renderer
    // only sees codecs FFmpeg can't decode. CPU cost is negligible on
    // modern Android TV silicon (~1–3 % for AAC stereo) and we're
    // always plugged in.
    //
    // `setEnableDecoderFallback(true)` is belt-and-braces: if the
    // selected renderer hits a runtime init failure (corrupted .so,
    // missing symbol on an exotic ABI, …), ExoPlayer transparently
    // retries with the next renderer instead of bubbling an error to
    // the user.
    val renderersFactory = DefaultRenderersFactory(context)
        .setExtensionRendererMode(DefaultRenderersFactory.EXTENSION_RENDERER_MODE_PREFER)
        .setEnableDecoderFallback(true)

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
        -> "The server refused this stream — you may need to sign in again."
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
        -> "Playback stalled — the decoder couldn't keep up."
        PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_WRITE_FAILED,
        -> "Audio output failed — check your HDMI / speaker connection."
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
@UnstableApi
fun buildMediaItem(
    playUrl: String,
    title: String? = null,
    mimeType: String = MimeTypes.APPLICATION_MATROSKA,
): MediaItem =
    MediaItem.Builder()
        .setUri(playUrl)
        .setMimeType(mimeType)
        .apply {
            if (!title.isNullOrBlank()) {
                setMediaMetadata(MediaMetadata.Builder().setTitle(title).build())
            }
        }
        .build()
