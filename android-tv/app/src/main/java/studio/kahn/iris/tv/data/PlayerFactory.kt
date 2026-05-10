package studio.kahn.iris.tv.data

import android.content.Context
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import okhttp3.OkHttpClient

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
    userAgent: String = "iris-tv/0.1.0 (Media3)",
): ExoPlayer {
    val dataSourceFactory = OkHttpDataSource.Factory(okHttp).setUserAgent(userAgent)
    val mediaSourceFactory = DefaultMediaSourceFactory(context)
        .setDataSourceFactory(dataSourceFactory)

    return ExoPlayer.Builder(context)
        .setMediaSourceFactory(mediaSourceFactory)
        .setSeekBackIncrementMs(10_000)
        .setSeekForwardIncrementMs(30_000)
        .build()
        .apply {
            // Without a wake mode the CPU goes to sleep on idle TV
            // hardware (no remote events for >2 min during a quiet
            // dialogue scene) and playback stalls. NETWORK keeps a
            // partial wake lock + WiFi lock while playing so HLS
            // segment fetches keep flowing. The lock is released
            // automatically when playback pauses or `release()` is
            // called.
            setWakeMode(C.WAKE_MODE_NETWORK)
        }
}

/**
 * Build a `MediaItem` for the Iris `/play/master.m3u8` endpoint — an
 * HLS-CMAF master playlist exposing one video variant + N audio
 * renditions. The `APPLICATION_M3U8` mime hint routes the
 * `DefaultMediaSourceFactory` to `HlsMediaSource`, which handles
 * multi-audio rendition switching natively via Media3's track selector.
 * Text-based subtitles are still side-loaded as external WebVTT tracks.
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
    subtitles: List<MediaItem.SubtitleConfiguration>,
): MediaItem =
    MediaItem.Builder()
        .setUri(playUrl)
        .setMimeType(MimeTypes.APPLICATION_M3U8)
        .setSubtitleConfigurations(subtitles)
        .build()
